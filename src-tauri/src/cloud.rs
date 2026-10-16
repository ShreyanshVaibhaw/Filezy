//! Cloud folders (F10): the sync roots that providers such as OneDrive and
//! Dropbox register with Windows, which Explorer lists in its navigation pane,
//! and the state of the items in them.
use serde::Serialize;
use windows::core::{Interface, HSTRING};
use windows::Storage::IStorageItem;
use windows::Storage::Provider::StorageProviderSyncRootManager;

#[derive(Serialize, Clone)]
pub struct Root {
    pub path: String,
    /// As Explorer names it: "OneDrive - Personal", "OneDrive - Contoso".
    pub name: String,
}

/// This user's sync roots; %OneDrive% when no provider registered any.
pub fn roots() -> Vec<Root> {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let mut out: Vec<Root> = registered().unwrap_or_default();
    out.retain(|r| std::path::Path::new(&r.path).is_dir());
    out.sort_by_key(|r| r.name.to_lowercase());
    if out.is_empty() {
        if let Some(p) = std::env::var("OneDrive").ok().filter(|p| std::path::Path::new(p).is_dir()) {
            out.push(Root { path: p, name: "OneDrive".into() });
        }
    }
    out
}

fn registered() -> windows::core::Result<Vec<Root>> {
    let list = StorageProviderSyncRootManager::GetCurrentSyncRoots()?;
    let mut out = Vec::new();
    for i in 0..list.Size()? {
        let info = list.GetAt(i)?;
        let Ok(path) = info.Path().and_then(|f| f.cast::<IStorageItem>()).and_then(|f| f.Path()) else { continue };
        let name = info.DisplayNameResource().map(|n| indirect(&n)).unwrap_or_default();
        let path = path.to_string();
        let name = if name.is_empty() { crate::paths::file_name(&path).to_string() } else { name };
        out.push(Root { path, name });
    }
    Ok(out)
}

/// Whether `dir` is in a folder a cloud provider syncs. Asks the cloud files
/// API, which knows every provider, including ones that don't list themselves
/// among the sync roots. Sync roots are local, so network paths aren't asked.
pub fn synced(dir: &str) -> bool {
    use windows::Win32::Storage::CloudFilters::{CfGetSyncRootInfoByPath, CF_SYNC_ROOT_INFO_BASIC};
    if dir.starts_with(r"\\") {
        return false;
    }
    let w = crate::fs_list::wide(&crate::paths::to_native(dir));
    let mut id = 0i64; // CF_SYNC_ROOT_BASIC_INFO: the root's file id
    unsafe { CfGetSyncRootInfoByPath(windows::core::PCWSTR(w.as_ptr()), CF_SYNC_ROOT_INFO_BASIC, &mut id as *mut i64 as *mut _, 8, None) }.is_ok()
}

/// Item attributes that can mean a cloud state: pinned, unpinned and
/// online-only items carry their own bits, and Windows shows some apps a
/// placeholder's reparse bit. Outside synced folders, ordinary files have none,
/// so the shell isn't asked about them. Inside one, a downloaded file can look
/// ordinary to Filezy, so callers ask about every item there.
pub fn may_have_state(attrs: u32) -> bool {
    attrs & (0x400 | 0x1000 | 0x4_0000 | 0x8_0000 | 0x10_0000 | 0x40_0000) != 0
}

/// Resolve "@dll,-id" style resource strings; plain text passes through.
fn indirect(s: &HSTRING) -> String {
    let text = s.to_string();
    if !text.starts_with('@') {
        return text;
    }
    let mut buf = [0u16; 512];
    match unsafe { windows::Win32::UI::Shell::SHLoadIndirectString(s, &mut buf, None) } {
        Ok(()) => String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_are_folders_with_names() {
        // Whatever this machine has: each root is a real folder with a name.
        for r in roots() {
            assert!(std::path::Path::new(&r.path).is_dir(), "{}", r.path);
            assert!(!r.name.is_empty());
        }
    }
}
