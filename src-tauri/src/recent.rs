//! Recent files for the Home page: resolve the .lnk shortcuts Windows keeps in
//! %APPDATA%\Microsoft\Windows\Recent via IShellLink.
use crate::fs_list::wide;
use serde::Serialize;
use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

#[derive(Serialize)]
pub struct RecentFile {
    pub name: String,
    pub path: String,
    pub ext: String,
    pub modified: i64,
}

thread_local! {
    static COM_INIT: () = unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); };
}

fn resolve_lnk(link: &IShellLinkW, lnk_path: &str) -> Option<String> {
    unsafe {
        let pf: IPersistFile = link.cast().ok()?;
        let w = wide(lnk_path);
        pf.Load(PCWSTR(w.as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 260];
        // No Resolve(): it can pop UI or hit the network for dead links.
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let len = buf.iter().position(|&c| c == 0)?;
        if len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

pub fn list(max: usize) -> Vec<RecentFile> {
    COM_INIT.with(|_| {});
    let Ok(appdata) = std::env::var("APPDATA") else { return Vec::new() };
    let dir = std::path::Path::new(&appdata).join("Microsoft\\Windows\\Recent");
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let link: IShellLinkW =
        match unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) } {
            Ok(l) => l,
            Err(_) => return Vec::new(),
        };

    // Newest shortcuts first, so we resolve as few .lnk files as possible.
    let mut lnks: Vec<(std::path::PathBuf, std::time::SystemTime)> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
        .filter_map(|e| {
            let t = e.metadata().ok()?.modified().ok()?;
            Some((e.path(), t))
        })
        .collect();
    lnks.sort_by(|a, b| b.1.cmp(&a.1));

    let mut out = Vec::new();
    for (lnk, t) in lnks {
        if out.len() >= max {
            break;
        }
        let Some(target) = resolve_lnk(&link, &lnk.to_string_lossy()) else { continue };
        let p = std::path::Path::new(&target);
        // Explorer's Recent files list shows files only, and only ones that still exist.
        if !p.is_file() {
            continue;
        }
        let modified = t
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        out.push(RecentFile {
            name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            path: target.clone(),
            ext: p
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default(),
            modified,
        });
    }
    out
}
