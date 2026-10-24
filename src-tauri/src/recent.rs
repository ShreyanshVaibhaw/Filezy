//! Recent files for the Home page: resolve the .lnk shortcuts Windows keeps in
//! %APPDATA%\Microsoft\Windows\Recent via IShellLink.
use crate::fs_list::wide;
use serde::Serialize;
use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

#[derive(Serialize)]
pub struct RecentFile {
    pub name: String,
    pub path: String,
    pub ext: String,
    pub modified: i64,
}

fn resolve_lnk(link: &IShellLinkW, lnk_path: &str) -> Option<String> {
    unsafe {
        let pf: IPersistFile = link.cast().ok()?;
        let w = wide(lnk_path);
        pf.Load(PCWSTR(w.as_ptr()), STGM_READ).ok()?;
        // Room for extended-length targets, not just MAX_PATH.
        let mut buf = vec![0u16; 32_768];
        // No Resolve(): it can pop UI or hit the network for dead links.
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let len = buf.iter().position(|&c| c == 0)?;
        if len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

/// Windows' Recent items folder for this account. Only its own .lnk files
/// are Recent files; the jump-list folders inside it are other apps' lists.
fn dir() -> Option<std::path::PathBuf> {
    Some(std::path::Path::new(&std::env::var("APPDATA").ok()?).join("Microsoft\\Windows\\Recent"))
}

/// The shortcuts in Recent items, newest first.
fn shortcuts() -> Vec<(std::path::PathBuf, std::time::SystemTime)> {
    let Some(Ok(rd)) = dir().map(std::fs::read_dir) else { return Vec::new() };
    let mut lnks: Vec<(std::path::PathBuf, std::time::SystemTime)> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
        .filter_map(|e| {
            let t = e.metadata().ok()?.modified().ok()?;
            Some((e.path(), t))
        })
        .collect();
    lnks.sort_by_key(|a| std::cmp::Reverse(a.1));
    lnks
}

fn shell_link() -> Option<IShellLinkW> {
    crate::com::sta();
    unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.ok()
}

/// Take these files off Recent files: delete the shortcuts in Recent items
/// that point at them, and nothing else. Returns how many went.
pub fn remove(paths: &[String]) -> Result<usize, String> {
    let link = shell_link().ok_or("Windows' shortcut reader isn't available.")?;
    let mut n = 0;
    for (lnk, _) in shortcuts() {
        let Some(target) = resolve_lnk(&link, &lnk.to_string_lossy()) else { continue };
        let target = target.trim_end_matches('\\').to_lowercase();
        if paths.iter().any(|p| p.trim_end_matches('\\').to_lowercase() == target) {
            std::fs::remove_file(&lnk).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

/// Clear Recent files: delete every shortcut in Recent items. Jump lists and
/// everything else stay. Returns how many went.
pub fn clear() -> Result<usize, String> {
    let lnks = shortcuts();
    for (lnk, _) in &lnks {
        std::fs::remove_file(lnk).map_err(|e| e.to_string())?;
    }
    Ok(lnks.len())
}

pub fn list(max: usize) -> Vec<RecentFile> {
    let Some(link) = shell_link() else { return Vec::new() };
    // Newest shortcuts first, so we resolve as few .lnk files as possible.
    let lnks = shortcuts();

    let mut out = Vec::new();
    // Two shortcuts can point at one file; the newest stands for it.
    let mut seen = std::collections::HashSet::new();
    for (lnk, t) in lnks {
        if out.len() >= max {
            break;
        }
        let Some(target) = resolve_lnk(&link, &lnk.to_string_lossy()) else { continue };
        let p = std::path::Path::new(&target);
        // Explorer's Recent files list shows files only, and only ones that still exist.
        if !p.is_file() || !seen.insert(crate::paths::identity(&target)) {
            continue;
        }
        let modified = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        out.push(RecentFile {
            name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            path: target.clone(),
            ext: p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default(),
            modified,
        });
    }
    out
}
