//! Optional tools (O01): checksums, byte comparison, folder sizes and the
//! side-by-side window pair. Each long job carries a number from the page;
//! `cancel_job` stops it between reads, and progress goes out as events.
use serde::Serialize;
use std::collections::HashSet;
use std::io::Read;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use windows::Win32::Security::Cryptography::{
    BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData, BCRYPT_ALG_HANDLE, BCRYPT_HASH_HANDLE, BCRYPT_MD5_ALG_HANDLE,
    BCRYPT_SHA1_ALG_HANDLE, BCRYPT_SHA256_ALG_HANDLE, BCRYPT_SHA512_ALG_HANDLE,
};

use crate::paths;

static CANCELLED: Mutex<Option<HashSet<u32>>> = Mutex::new(None);
const CHUNK: usize = 1 << 20;
const TICK: Duration = Duration::from_millis(150);

fn cancelled(job: u32) -> bool {
    CANCELLED.lock().unwrap().as_ref().is_some_and(|s| s.contains(&job))
}
fn finish(job: u32) {
    if let Some(s) = CANCELLED.lock().unwrap().as_mut() {
        s.remove(&job);
    }
}

#[tauri::command]
pub fn cancel_job(job: u32) {
    CANCELLED.lock().unwrap().get_or_insert_with(HashSet::new).insert(job);
}

// ---- checksums ----

/// One running hash from Windows' CNG, through its pseudo-handles (no provider to open or close).
struct Hasher {
    name: &'static str,
    handle: BCRYPT_HASH_HANDLE,
    len: usize,
}

impl Hasher {
    fn new(name: &str) -> Result<Hasher, String> {
        let (name, alg, len): (&'static str, BCRYPT_ALG_HANDLE, usize) = match name {
            "MD5" => ("MD5", BCRYPT_MD5_ALG_HANDLE, 16),
            "SHA-1" => ("SHA-1", BCRYPT_SHA1_ALG_HANDLE, 20),
            "SHA-256" => ("SHA-256", BCRYPT_SHA256_ALG_HANDLE, 32),
            "SHA-512" => ("SHA-512", BCRYPT_SHA512_ALG_HANDLE, 64),
            other => return Err(format!("Unknown algorithm {other}.")),
        };
        let mut handle = BCRYPT_HASH_HANDLE::default();
        unsafe { BCryptCreateHash(alg, &mut handle, None, None, 0) }.ok().map_err(|e| e.message())?;
        Ok(Hasher { name, handle, len })
    }
    fn update(&self, data: &[u8]) -> Result<(), String> {
        unsafe { BCryptHashData(self.handle, data, 0) }.ok().map_err(|e| e.message())
    }
    fn hex(&self) -> Result<String, String> {
        let mut out = vec![0u8; self.len];
        unsafe { BCryptFinishHash(self.handle, &mut out, 0) }.ok().map_err(|e| e.message())?;
        Ok(out.iter().map(|b| format!("{b:02x}")).collect())
    }
}

impl Drop for Hasher {
    fn drop(&mut self) {
        unsafe {
            let _ = BCryptDestroyHash(self.handle);
        }
    }
}

#[derive(Serialize, Clone)]
pub struct Progress {
    job: u32,
    done: u64,
    total: u64,
}

#[derive(Serialize)]
pub struct HashResult {
    path: String,
    size: u64,
    /// (algorithm, lowercase hex), in the order asked for.
    hashes: Vec<(String, String)>,
    error: Option<String>,
}

/// Every chosen algorithm over one read of each file.
fn hash_one(path: &str, algos: &[String], tick: &mut dyn FnMut(u64) -> bool) -> Result<Vec<(String, String)>, String> {
    let hashers = algos.iter().map(|a| Hasher::new(a)).collect::<Result<Vec<_>, _>>()?;
    let mut f = std::fs::File::open(paths::to_native(path)).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        for h in &hashers {
            h.update(&buf[..n])?;
        }
        if !tick(n as u64) {
            return Err("Stopped.".into());
        }
    }
    hashers.iter().map(|h| Ok((h.name.to_string(), h.hex()?))).collect()
}

#[tauri::command]
pub async fn hash_files(job: u32, paths: Vec<String>, algos: Vec<String>, app: AppHandle) -> Result<Vec<HashResult>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let sizes: Vec<u64> = paths.iter().map(|p| std::fs::metadata(paths::to_native(p)).map(|m| m.len()).unwrap_or(0)).collect();
        let total = sizes.iter().sum();
        let mut done = 0u64;
        let mut last = Instant::now();
        let mut out = Vec::new();
        for (p, &size) in paths.iter().zip(&sizes) {
            if cancelled(job) {
                break;
            }
            let mut tick = |n: u64| {
                done += n;
                if last.elapsed() > TICK {
                    last = Instant::now();
                    let _ = app.emit("tool-progress", Progress { job, done, total });
                }
                !cancelled(job)
            };
            let r = hash_one(p, &algos, &mut tick);
            let (hashes, error) = match r {
                Ok(h) => (h, None),
                Err(e) => (vec![], Some(e)),
            };
            out.push(HashResult { path: p.clone(), size, hashes, error });
        }
        finish(job);
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---- byte comparison ----

#[derive(Serialize, Debug, PartialEq)]
pub struct Compared {
    same: bool,
    size_a: u64,
    size_b: u64,
    /// Offset of the first byte that differs (the shorter length when one file is a prefix of the other).
    first_diff: Option<u64>,
}

fn compare(a: &str, b: &str, tick: &mut dyn FnMut(u64) -> bool) -> Result<Compared, String> {
    let open = |p: &str| std::fs::File::open(paths::to_native(p)).map_err(|e| format!("{}: {e}", paths::file_name(p)));
    let (mut fa, mut fb) = (open(a)?, open(b)?);
    let size_a = fa.metadata().map_err(|e| e.to_string())?.len();
    let size_b = fb.metadata().map_err(|e| e.to_string())?.len();
    let (mut ba, mut bb) = (vec![0u8; CHUNK], vec![0u8; CHUNK]);
    let mut pos = 0u64;
    loop {
        let na = read_full(&mut fa, &mut ba)?;
        let nb = read_full(&mut fb, &mut bb)?;
        let n = na.min(nb);
        if let Some(i) = ba[..n].iter().zip(&bb[..n]).position(|(x, y)| x != y) {
            return Ok(Compared { same: false, size_a, size_b, first_diff: Some(pos + i as u64) });
        }
        if na != nb {
            return Ok(Compared { same: false, size_a, size_b, first_diff: Some(pos + n as u64) });
        }
        if na == 0 {
            return Ok(Compared { same: true, size_a, size_b, first_diff: None });
        }
        pos += n as u64;
        if !tick(n as u64) {
            return Err("Stopped.".into());
        }
    }
}

/// Fill the buffer unless the file ends, so both sides advance in step.
fn read_full(f: &mut std::fs::File, buf: &mut [u8]) -> Result<usize, String> {
    let mut got = 0;
    while got < buf.len() {
        match f.read(&mut buf[got..]).map_err(|e| e.to_string())? {
            0 => break,
            n => got += n,
        }
    }
    Ok(got)
}

#[tauri::command]
pub async fn compare_files(job: u32, a: String, b: String, app: AppHandle) -> Result<Compared, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let total = std::fs::metadata(paths::to_native(&a)).map(|m| m.len()).unwrap_or(0);
        let mut done = 0;
        let mut last = Instant::now();
        let r = compare(&a, &b, &mut |n| {
            done += n;
            if last.elapsed() > TICK {
                last = Instant::now();
                let _ = app.emit("tool-progress", Progress { job, done, total });
            }
            !cancelled(job)
        });
        finish(job);
        r
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---- folder sizes ----

#[derive(Serialize, Clone, Default, Debug)]
pub struct Size {
    job: u32,
    path: String,
    is_dir: bool,
    bytes: u64,
    files: u64,
    folders: u64,
    /// Folders that couldn't be read (access denied, gone meanwhile).
    skipped: u64,
    /// Junctions and symbolic links, counted but never followed, so a link
    /// back to an ancestor can't loop and nothing is counted twice through one.
    links: u64,
    done: bool,
}

/// Walk one item. Hands out partial totals through `tick`; false from it stops the walk.
fn measure(path: &str, s: &mut Size, tick: &mut dyn FnMut(&Size) -> bool) {
    let native = paths::to_native(path);
    let Ok(meta) = std::fs::symlink_metadata(&native) else {
        s.skipped += 1;
        return;
    };
    if meta.file_type().is_symlink() {
        s.links += 1;
        return;
    }
    if !meta.is_dir() {
        s.files += 1;
        s.bytes += meta.len();
        return;
    }
    s.is_dir = true;
    let mut stack = vec![std::path::PathBuf::from(native)];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            s.skipped += 1;
            continue;
        };
        for e in rd.flatten() {
            // On Windows both come from the directory listing itself: no extra open per file.
            let (Ok(ft), Ok(m)) = (e.file_type(), e.metadata()) else { continue };
            if ft.is_symlink() {
                s.links += 1;
            } else if ft.is_dir() {
                s.folders += 1;
                stack.push(e.path());
            } else {
                s.files += 1;
                s.bytes += m.len();
            }
        }
        if !tick(s) {
            return;
        }
    }
}

/// Sizes of `paths`, or with `dir` of each item directly inside it (the storage
/// view). Each item's total goes out as a "tool-size" event when it's done, and
/// partial totals meanwhile; the call returns when all are done or it's stopped.
#[tauri::command]
pub async fn measure_sizes(job: u32, dir: Option<String>, paths: Vec<String>, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let items: Vec<String> = match dir {
            Some(d) => std::fs::read_dir(paths::to_native(&d))
                .map_err(|e| e.to_string())?
                .flatten()
                .map(|e| paths::join(&d, &e.file_name().to_string_lossy()))
                .collect(),
            None => paths,
        };
        let mut last = Instant::now();
        for p in items {
            if cancelled(job) {
                break;
            }
            let mut s = Size { job, path: p.clone(), ..Default::default() };
            measure(&p, &mut s, &mut |s| {
                if last.elapsed() > TICK {
                    last = Instant::now();
                    let _ = app.emit("tool-size", s.clone());
                }
                !cancelled(job)
            });
            s.done = !cancelled(job);
            let _ = app.emit("tool-size", s);
        }
        finish(job);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---- dual pane: two windows side by side ----

/// Halves of the work area of the monitor `hwnd` is on, in screen pixels.
fn halves(hwnd: isize) -> Option<(windows::Win32::Foundation::RECT, windows::Win32::Foundation::RECT)> {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(MonitorFromWindow(HWND(hwnd as *mut _), MONITOR_DEFAULTTONEAREST), &mut mi) }.ok().ok()?;
    let w = mi.rcWork;
    let mid = w.left + (w.right - w.left) / 2;
    Some((RECT { right: mid, ..w }, RECT { left: mid, ..w }))
}

fn place(hwnd: isize, r: windows::Win32::Foundation::RECT) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, ShowWindow, SWP_NOZORDER, SWP_SHOWWINDOW, SW_RESTORE};
    let h = HWND(hwnd as *mut _);
    unsafe {
        let _ = ShowWindow(h, SW_RESTORE);
        let _ = SetWindowPos(h, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_SHOWWINDOW);
    }
}

/// Put this window on the left half of its monitor and open `path` in a new
/// window on the right half: two independent locations on the one operation engine.
#[tauri::command]
pub fn split_view(path: Option<String>, app: AppHandle) -> Result<(), String> {
    let hwnd = crate::owner(&app);
    let (left, right) = halves(hwnd).ok_or("The monitor's work area could not be read.")?;
    place(hwnd, left);
    let mut cmd = std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    if let Some(p) = path {
        cmd.arg(p);
    }
    cmd.arg(format!("--filezy-snap={},{},{},{}", right.left, right.top, right.right, right.bottom));
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// The rectangle a window opened by split_view takes, from its command line.
pub fn snap_arg() -> Option<windows::Win32::Foundation::RECT> {
    let a = std::env::args().find_map(|a| a.strip_prefix("--filezy-snap=").map(str::to_string))?;
    let v: Vec<i32> = a.split(',').map(|n| n.parse().ok()).collect::<Option<_>>()?;
    let [left, top, right, bottom] = v[..] else { return None };
    (right - left >= 100 && bottom - top >= 100).then_some(windows::Win32::Foundation::RECT { left, top, right, bottom })
}

pub fn apply_snap(hwnd: isize) -> bool {
    snap_arg().map(|r| place(hwnd, r)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, data: &[u8]) -> String {
        let p = std::env::temp_dir().join(format!("filezy-tools-{}-{name}", std::process::id()));
        std::fs::write(&p, data).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn hashes_match_known_vectors() {
        let p = tmp("abc", b"abc");
        let all: Vec<String> = ["MD5", "SHA-1", "SHA-256"].map(String::from).to_vec();
        let h = hash_one(&p, &all, &mut |_| true).unwrap();
        assert_eq!(h[0].1, "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(h[1].1, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(h[2].1, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(hash_one(&p, &["CRC".into()], &mut |_| true).is_err());
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn compare_finds_first_difference() {
        let big: Vec<u8> = (0..CHUNK + 10).map(|i| i as u8).collect();
        let mut other = big.clone();
        other[CHUNK + 3] ^= 1;
        let (a, b, c) = (tmp("a", &big), tmp("b", &other), tmp("c", &big[..100]));
        let d = tmp("d", &big);
        assert_eq!(compare(&a, &d, &mut |_| true).unwrap().first_diff, None);
        assert_eq!(compare(&a, &b, &mut |_| true).unwrap().first_diff, Some(CHUNK as u64 + 3));
        let r = compare(&a, &c, &mut |_| true).unwrap();
        assert_eq!((r.same, r.first_diff), (false, Some(100)));
        for p in [a, b, c, d] {
            std::fs::remove_file(p).unwrap();
        }
    }

    #[test]
    fn sizes_count_files_and_skip_links() {
        let root = std::env::temp_dir().join(format!("filezy-sizes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.bin"), [0u8; 10]).unwrap();
        std::fs::write(root.join("sub").join("b.bin"), [0u8; 5]).unwrap();
        // A junction back to the root: must be counted, not walked.
        let made = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(root.join("sub").join("loop"))
            .arg(&root)
            .output()
            .is_ok_and(|o| o.status.success());
        let mut s = Size::default();
        measure(&root.to_string_lossy(), &mut s, &mut |_| true);
        assert_eq!((s.bytes, s.files, s.folders), (15, 2, 1));
        assert_eq!(s.links, made as u64);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
