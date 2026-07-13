use crate::Entry;
use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::Storage::FileSystem::{
    FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW,
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM,
    FIND_FIRST_EX_LARGE_FETCH, WIN32_FIND_DATAW,
};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Resolve to an absolute, prefix-free path ("C:\Users\x").
pub fn normalize(path: &str) -> Result<String, String> {
    let p = path.trim();
    let p = if p.len() == 2 && p.ends_with(':') { format!("{p}\\") } else { p.to_string() };
    let canon = std::fs::canonicalize(&p).map_err(|e| format!("{p}: {e}"))?;
    let s = canon.to_string_lossy().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
    Ok(s)
}

const FILETIME_UNIX_EPOCH: u64 = 116_444_736_000_000_000;

pub fn list(dir: &str) -> Result<Vec<Entry>, String> {
    let pattern = wide(&format!(r"\\?\{}\*", dir.trim_end_matches('\\')));
    let mut data = WIN32_FIND_DATAW::default();
    let handle = unsafe {
        FindFirstFileExW(
            PCWSTR(pattern.as_ptr()),
            FindExInfoBasic,
            &mut data as *mut _ as *mut _,
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        )
    }
    .map_err(|e| {
        if e.code() == ERROR_FILE_NOT_FOUND.into() {
            "empty".to_string()
        } else {
            e.message().to_string()
        }
    });

    let handle = match handle {
        Ok(h) => h,
        Err(e) if e == "empty" => return Ok(vec![]),
        Err(e) => return Err(e),
    };

    let mut entries = Vec::with_capacity(256);
    loop {
        let len = data.cFileName.iter().position(|&c| c == 0).unwrap_or(260);
        let name = String::from_utf16_lossy(&data.cFileName[..len]);
        if name != "." && name != ".." {
            let attrs = data.dwFileAttributes;
            let is_dir = attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
            let hidden =
                attrs & FILE_ATTRIBUTE_HIDDEN.0 != 0 || attrs & FILE_ATTRIBUTE_SYSTEM.0 != 0;
            let size = ((data.nFileSizeHigh as u64) << 32) | data.nFileSizeLow as u64;
            let ft = ((data.ftLastWriteTime.dwHighDateTime as u64) << 32)
                | data.ftLastWriteTime.dwLowDateTime as u64;
            let modified = ft.saturating_sub(FILETIME_UNIX_EPOCH) / 10_000; // ms since unix epoch
            let ext = if is_dir {
                String::new()
            } else {
                name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
            };
            entries.push(Entry { name, is_dir, size, modified, ext, hidden, parent: None });
        }
        if unsafe { FindNextFileW(handle, &mut data) }.is_err() {
            break;
        }
    }
    unsafe { FindClose(handle).ok() };
    Ok(entries)
}
