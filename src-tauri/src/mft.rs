//! Whole-volume name search from the NTFS MFT (the "Everything" technique).
//!
//! FSCTL_ENUM_USN_DATA builds an in-memory (name, parent) index of a volume;
//! queries scan it and rebuild full paths from the parent chain. Before each
//! search the index replays the volume's USN change journal since it was built,
//! so creates, deletes and renames made meanwhile are reflected.
//!
//! Opening the raw volume (`\\.\C:`) needs elevation. Without it, `search`
//! returns false and the caller walks the folder tree instead.
use crate::listing::{self, Entry};
use crate::query::Query;
use crate::search::{SearchReq, Sink};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Ioctl::{
    FSCTL_ENUM_USN_DATA, FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_USN_JOURNAL, MFT_ENUM_DATA_V0,
    READ_USN_JOURNAL_DATA_V0, USN_JOURNAL_DATA_V0, USN_RECORD_V2,
};
use windows::Win32::System::IO::DeviceIoControl;

use crate::fs_list::wide;

const USN_REASON_FILE_CREATE: u32 = 0x100;
const USN_REASON_FILE_DELETE: u32 = 0x200;
const USN_REASON_RENAME_OLD_NAME: u32 = 0x1000;
const USN_REASON_RENAME_NEW_NAME: u32 = 0x2000;

struct Node {
    name: String,
    parent: u64,
    attrs: u32,
}

struct VolumeIndex {
    letter: char,
    nodes: HashMap<u64, Node>,
    journal: u64,
    next_usn: i64,
}

fn cache() -> &'static Mutex<HashMap<char, VolumeIndex>> {
    static C: OnceLock<Mutex<HashMap<char, VolumeIndex>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// True when `root` is a bare drive root ("C:\") whose volume might be indexable.
pub fn can_handle(root: &str) -> bool {
    let r = root.trim_end_matches('\\');
    r.len() == 2 && r.as_bytes()[1] == b':'
}

struct Volume(HANDLE);
impl Drop for Volume {
    fn drop(&mut self) {
        unsafe { let _ = CloseHandle(self.0); }
    }
}

fn open_volume(letter: char) -> Option<Volume> {
    let path = wide(&format!(r"\\.\{letter}:"));
    unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
        .ok()
        .map(Volume)
    }
}

fn query_journal(v: &Volume) -> Option<USN_JOURNAL_DATA_V0> {
    let mut data = USN_JOURNAL_DATA_V0::default();
    let mut got = 0u32;
    unsafe {
        DeviceIoControl(
            v.0,
            FSCTL_QUERY_USN_JOURNAL,
            None,
            0,
            Some(&mut data as *mut _ as *mut c_void),
            std::mem::size_of::<USN_JOURNAL_DATA_V0>() as u32,
            Some(&mut got),
            None,
        )
        .ok()?;
    }
    Some(data)
}

/// Visit each USN_RECORD_V2 in `buf[8..len]` (the first 8 bytes are a cursor).
fn records(buf: &[u8], len: usize, mut f: impl FnMut(&USN_RECORD_V2, String)) {
    let mut off = 8usize;
    while off + std::mem::size_of::<USN_RECORD_V2>() <= len {
        let rec = unsafe { &*(buf.as_ptr().add(off) as *const USN_RECORD_V2) };
        let rec_len = rec.RecordLength as usize;
        if rec_len == 0 || off + rec_len > len {
            break;
        }
        let name_off = rec.FileNameOffset as usize;
        let name_len = rec.FileNameLength as usize / 2;
        if off + name_off + name_len * 2 <= len {
            let name = unsafe {
                let p = (rec as *const USN_RECORD_V2 as *const u8).add(name_off) as *const u16;
                String::from_utf16_lossy(std::slice::from_raw_parts(p, name_len))
            };
            f(rec, name);
        }
        off += rec_len;
    }
}

fn build(letter: char) -> Option<VolumeIndex> {
    let v = open_volume(letter)?;
    // Journal position first, so changes made during enumeration get replayed.
    let journal = query_journal(&v)?;
    let mut nodes: HashMap<u64, Node> = HashMap::new();
    let mut med = MFT_ENUM_DATA_V0 { StartFileReferenceNumber: 0, LowUsn: 0, HighUsn: i64::MAX };
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                v.0,
                FSCTL_ENUM_USN_DATA,
                Some(&med as *const _ as *const c_void),
                std::mem::size_of::<MFT_ENUM_DATA_V0>() as u32,
                Some(buf.as_mut_ptr() as *mut c_void),
                buf.len() as u32,
                Some(&mut returned),
                None,
            )
        };
        // ERROR_HANDLE_EOF (or any error) ends enumeration.
        if ok.is_err() || returned as usize <= 8 {
            break;
        }
        med.StartFileReferenceNumber = u64::from_le_bytes(buf[0..8].try_into().unwrap());
        records(&buf, returned as usize, |r, name| {
            nodes.insert(r.FileReferenceNumber, Node { name, parent: r.ParentFileReferenceNumber, attrs: r.FileAttributes });
        });
    }
    (!nodes.is_empty()).then_some(VolumeIndex { letter, nodes, journal: journal.UsnJournalID, next_usn: journal.NextUsn })
}

impl VolumeIndex {
    /// Apply journal records written since the index was built or last caught up.
    /// False when the journal was recreated or has wrapped past our position.
    fn catch_up(&mut self) -> bool {
        let Some(v) = open_volume(self.letter) else { return false };
        let Some(j) = query_journal(&v) else { return false };
        if j.UsnJournalID != self.journal || j.FirstUsn > self.next_usn {
            return false;
        }
        let mut buf = vec![0u8; 1 << 16];
        while self.next_usn < j.NextUsn {
            let req = READ_USN_JOURNAL_DATA_V0 {
                StartUsn: self.next_usn,
                ReasonMask: 0xFFFF_FFFF,
                ReturnOnlyOnClose: 0,
                Timeout: 0,
                BytesToWaitFor: 0,
                UsnJournalID: self.journal,
            };
            let mut returned = 0u32;
            let ok = unsafe {
                DeviceIoControl(
                    v.0,
                    FSCTL_READ_USN_JOURNAL,
                    Some(&req as *const _ as *const c_void),
                    std::mem::size_of::<READ_USN_JOURNAL_DATA_V0>() as u32,
                    Some(buf.as_mut_ptr() as *mut c_void),
                    buf.len() as u32,
                    Some(&mut returned),
                    None,
                )
            };
            if ok.is_err() || (returned as usize) < 8 {
                return false;
            }
            let next = i64::from_le_bytes(buf[0..8].try_into().unwrap());
            records(&buf, returned as usize, |r, name| {
                let frn = r.FileReferenceNumber;
                if r.Reason & USN_REASON_FILE_DELETE != 0 {
                    self.nodes.remove(&frn);
                } else if r.Reason & USN_REASON_RENAME_OLD_NAME == 0
                    && (r.Reason & (USN_REASON_FILE_CREATE | USN_REASON_RENAME_NEW_NAME) != 0 || self.nodes.contains_key(&frn))
                {
                    self.nodes.insert(frn, Node { name, parent: r.ParentFileReferenceNumber, attrs: r.FileAttributes });
                }
            });
            if next <= self.next_usn {
                break;
            }
            self.next_usn = next;
        }
        true
    }

    /// Full parent directory path for a node ("C:\Users\x"). None if the chain breaks.
    fn dir_of(&self, mut frn: u64) -> Option<String> {
        let mut parts: Vec<&str> = Vec::new();
        while let Some(node) = self.nodes.get(&frn) {
            parts.push(&node.name);
            frn = node.parent;
            if parts.len() > 512 {
                return None; // cycle guard
            }
        }
        let mut path = format!("{}:", self.letter);
        for p in parts.iter().rev() {
            path.push('\\');
            path.push_str(p);
        }
        Some(if path.len() == 2 { format!("{path}\\") } else { path })
    }

    /// Hidden ancestors hide their contents when hidden items are off, like the walk.
    fn chain_visible(&self, mut frn: u64, show_hidden: bool, show_protected: bool) -> bool {
        let mut guard = 0;
        while let Some(node) = self.nodes.get(&frn) {
            if !listing::visible(node.attrs, show_hidden, show_protected) {
                return false;
            }
            frn = node.parent;
            guard += 1;
            if guard > 512 {
                return false;
            }
        }
        true
    }
}

/// Search the volume holding `req.root`. False if the volume can't be indexed,
/// so the caller falls back to the recursive walk.
pub fn search(app: &AppHandle, req: &SearchReq, q: &Query, note: &str) -> bool {
    let Some(letter) = req.root.chars().next().map(|c| c.to_ascii_uppercase()) else { return false };
    let mut cache = cache().lock().unwrap();
    let fresh = cache.get_mut(&letter).is_some_and(VolumeIndex::catch_up);
    if !fresh {
        cache.remove(&letter);
        let Some(built) = build(letter) else { return false };
        cache.insert(letter, built);
    }
    let index = &cache[&letter];
    let mut sink = Sink::new(app.clone(), req.seq, "mft");
    sink.update.note = Some(note.into());
    if sink.cancelled() {
        return true;
    }
    for node in index.nodes.values() {
        let name = node.name.to_lowercase();
        if !q.words.iter().all(|w| name.contains(w.as_str())) || !listing::visible(node.attrs, req.show_hidden, req.show_protected) {
            continue;
        }
        if !index.chain_visible(node.parent, req.show_hidden, req.show_protected) {
            continue;
        }
        let Some(dir) = index.dir_of(node.parent) else { continue };
        // USN records omit size and times; stat the match. A match that no longer
        // exists is dropped rather than shown with empty details.
        let full = crate::paths::join(&dir, &node.name);
        let Ok(meta) = std::fs::symlink_metadata(crate::paths::to_native(&full)) else { continue };
        let ms = |t: std::io::Result<std::time::SystemTime>| {
            t.ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        };
        let is_dir = node.attrs & listing::ATTR_DIRECTORY != 0;
        let ext = if is_dir {
            String::new()
        } else {
            node.name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
        };
        let e = Entry {
            name: node.name.clone(),
            is_dir,
            size: if is_dir { 0 } else { meta.len() },
            modified: ms(meta.modified()),
            created: ms(meta.created()),
            ext,
            attrs: node.attrs,
            parent: Some(dir),
        };
        if !sink.push(e) {
            break;
        }
    }
    if !sink.update.done {
        sink.flush(true);
    }
    true
}
