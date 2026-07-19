//! Machine-wide instant name search via the NTFS MFT (the "Everything" technique).
//!
//! Enumerates a volume's Master File Table with FSCTL_ENUM_USN_DATA to build an
//! in-memory (name, parent) index, then answers substring queries by scanning it
//! and reconstructing full paths from the parent chain.
//!
//! Opening the raw volume (`\\.\C:`) requires the process to be elevated; if the
//! open fails (the normal non-admin case) callers fall back to the recursive walk.
//! UNTESTED without elevation in this environment - guarded so a failure degrades
//! to the walk rather than breaking search.

use crate::{search, AppState, Entry};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tauri::{AppHandle, Manager};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Ioctl::{MFT_ENUM_DATA_V0, USN_RECORD_V2, FSCTL_ENUM_USN_DATA};
use windows::Win32::System::IO::DeviceIoControl;

use crate::fs_list::wide;

/// Cap on results returned per search, mirroring how Explorer streams a bounded set.
const MAX_RESULTS: usize = 20_000;

struct Node {
    name: String,
    parent: u64,
    is_dir: bool,
    hidden: bool,
}

pub struct VolumeIndex {
    letter: char,
    nodes: HashMap<u64, Node>,
}

fn cache() -> &'static Mutex<HashMap<char, Arc<VolumeIndex>>> {
    static C: OnceLock<Mutex<HashMap<char, Arc<VolumeIndex>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// True when `root` is a bare drive root ("C:\") whose volume we can index.
pub fn can_handle(root: &str) -> bool {
    let r = root.trim_end_matches('\\');
    r.len() == 2 && r.as_bytes()[1] == b':'
}

fn drive_letter(root: &str) -> Option<char> {
    let c = root.chars().next()?.to_ascii_uppercase();
    c.is_ascii_alphabetic().then_some(c)
}

/// Get the volume index, building (and caching) it on first use. None if the
/// volume can't be opened (not elevated / not NTFS).
// ponytail: the lock is released across build(), so two concurrent whole-drive
// searches on the same drive could each enumerate once (last write wins). Harmless
// (the index is idempotent) and rare; add a per-letter build lock if it ever bites.
fn get_or_build(letter: char) -> Option<Arc<VolumeIndex>> {
    if let Some(idx) = cache().lock().unwrap().get(&letter) {
        return Some(idx.clone());
    }
    let built = Arc::new(build(letter)?);
    cache().lock().unwrap().insert(letter, built.clone());
    Some(built)
}

fn build(letter: char) -> Option<VolumeIndex> {
    unsafe {
        let path = wide(&format!(r"\\.\{letter}:"));
        let handle = CreateFileW(
            PCWSTR(path.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
        .ok()?;

        let mut nodes: HashMap<u64, Node> = HashMap::new();
        let mut med = MFT_ENUM_DATA_V0 {
            StartFileReferenceNumber: 0,
            LowUsn: 0,
            HighUsn: i64::MAX,
        };
        let mut buf = vec![0u8; 1 << 16]; // 64 KiB fetch window

        loop {
            let mut returned: u32 = 0;
            let ok = DeviceIoControl(
                handle,
                FSCTL_ENUM_USN_DATA,
                Some(&med as *const _ as *const c_void),
                std::mem::size_of::<MFT_ENUM_DATA_V0>() as u32,
                Some(buf.as_mut_ptr() as *mut c_void),
                buf.len() as u32,
                Some(&mut returned),
                None,
            );
            // ERROR_HANDLE_EOF (or any error) ends enumeration.
            if ok.is_err() || (returned as usize) <= 8 {
                break;
            }
            let returned = returned as usize;
            // First 8 bytes: the next start FRN to resume from.
            med.StartFileReferenceNumber =
                u64::from_le_bytes(buf[0..8].try_into().unwrap());

            let mut off = 8usize;
            while off + std::mem::size_of::<USN_RECORD_V2>() <= returned {
                let rec = buf.as_ptr().add(off) as *const USN_RECORD_V2;
                let rec_len = (*rec).RecordLength as usize;
                if rec_len == 0 || off + rec_len > returned {
                    break;
                }
                let name_off = (*rec).FileNameOffset as usize;
                let name_len = (*rec).FileNameLength as usize / 2; // bytes -> u16 count
                if off + name_off + name_len * 2 <= returned {
                    let name_ptr = (rec as *const u8).add(name_off) as *const u16;
                    let name = String::from_utf16_lossy(std::slice::from_raw_parts(
                        name_ptr, name_len,
                    ));
                    let attrs = (*rec).FileAttributes;
                    nodes.insert(
                        (*rec).FileReferenceNumber,
                        Node {
                            name,
                            parent: (*rec).ParentFileReferenceNumber,
                            is_dir: attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
                            hidden: attrs & FILE_ATTRIBUTE_HIDDEN.0 != 0
                                || attrs & FILE_ATTRIBUTE_SYSTEM.0 != 0,
                        },
                    );
                }
                off += rec_len;
            }
        }

        let _ = CloseHandle(handle);
        if nodes.is_empty() {
            None
        } else {
            Some(VolumeIndex { letter, nodes })
        }
    }
}

impl VolumeIndex {
    /// Full parent directory path for a node ("C:\Users\x"). None if the chain breaks.
    fn dir_of(&self, mut frn: u64) -> Option<String> {
        // Walk up from the parent, prepending names until the chain leaves the map (root).
        let mut parts: Vec<&str> = Vec::new();
        let mut guard = 0;
        while let Some(node) = self.nodes.get(&frn) {
            parts.push(&node.name);
            frn = node.parent;
            guard += 1;
            if guard > 512 {
                return None; // cycle guard
            }
        }
        let mut path = format!("{}:\\", self.letter);
        for p in parts.iter().rev() {
            path.push_str(p);
            path.push('\\');
        }
        Some(path.trim_end_matches('\\').to_string())
    }
}

/// Case-insensitive ASCII-aware substring test without allocating.
fn contains_ci(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    let hb = haystack.as_bytes();
    let nb = needle_lower.as_bytes();
    if nb.len() > hb.len() {
        return false;
    }
    for start in 0..=hb.len() - nb.len() {
        if hb[start..start + nb.len()]
            .iter()
            .zip(nb)
            .all(|(&h, &n)| h.to_ascii_lowercase() == n)
        {
            return true;
        }
    }
    false
}

/// Run a machine-wide search over the volume containing `root`. Streams results
/// via the same "search-update" channel as the walk. Returns false if the volume
/// could not be indexed, so the caller falls back to the recursive walk.
pub fn search(app: &AppHandle, root: &str, query: &str, gen: u64) -> bool {
    let Some(letter) = drive_letter(root) else { return false };
    let Some(index) = get_or_build(letter) else { return false };
    if search::generation() != gen {
        return true; // superseded while building; treat as handled
    }

    let needle = query.to_lowercase();
    let state = app.state::<AppState>();
    let mut pending: Vec<Entry> = Vec::new();
    let mut count = 0usize;
    let mut last_flush = Instant::now();

    for (frn, node) in index.nodes.iter() {
        if search::generation() != gen {
            return true;
        }
        if !contains_ci(&node.name, &needle) {
            continue;
        }
        let Some(dir) = index.dir_of(node.parent) else { continue };
        let full = format!("{dir}\\{}", node.name);
        // USN records omit size/mtime; stat the match to fill them (matches are bounded).
        let (size, modified, created) = std::fs::metadata(&full)
            .map(|m| {
                let ms = |t: std::io::Result<std::time::SystemTime>| {
                    t.ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0)
                };
                (m.len(), ms(m.modified()), ms(m.created()))
            })
            .unwrap_or((0, 0, 0));
        let ext = if node.is_dir {
            String::new()
        } else {
            node.name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
        };
        pending.push(Entry {
            name: node.name.clone(),
            is_dir: node.is_dir,
            size,
            modified,
            created,
            ext,
            hidden: node.hidden,
            parent: Some(dir),
        });
        let _ = frn;
        count += 1;

        if last_flush.elapsed().as_millis() > 150 {
            if !flush(app, &state, &mut pending, gen, false) {
                return true;
            }
            last_flush = Instant::now();
        }
        if count >= MAX_RESULTS {
            break;
        }
    }
    flush(app, &state, &mut pending, gen, true);
    true
}

fn flush(
    app: &AppHandle,
    state: &AppState,
    pending: &mut Vec<Entry>,
    gen: u64,
    done: bool,
) -> bool {
    // Generation checked under the lock (see search.rs flush) so a newer navigation
    // that replaced the listing can't be clobbered by stale MFT results.
    let total = {
        let mut listing = state.listing.lock().unwrap();
        if search::generation() != gen {
            return false;
        }
        listing.entries.append(pending);
        listing.entries.len()
    };
    use tauri::Emitter;
    let _ = app.emit("search-update", search::SearchUpdate { total, done });
    true
}
