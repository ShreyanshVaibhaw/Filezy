use crate::listing::{Entry, ATTR_DIRECTORY};
use crate::paths;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_FILES};
use windows::Win32::Storage::FileSystem::{
    FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW,
    FIND_FIRST_EX_LARGE_FETCH, WIN32_FIND_DATAW,
};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

const FILETIME_UNIX_EPOCH: u64 = 116_444_736_000_000_000;

/// A FILETIME (100 ns ticks since 1601) as milliseconds since the Unix epoch.
pub fn filetime_ms(ft: u64) -> u64 {
    ft.saturating_sub(FILETIME_UNIX_EPOCH) / 10_000
}

fn filetime(ft: windows::Win32::Foundation::FILETIME) -> u64 {
    filetime_ms((ft.dwHighDateTime as u64) << 32 | ft.dwLowDateTime as u64)
}

pub fn entry_from(data: &WIN32_FIND_DATAW) -> Option<Entry> {
    let len = data.cFileName.iter().position(|&c| c == 0).unwrap_or(data.cFileName.len());
    let name = String::from_utf16_lossy(&data.cFileName[..len]);
    if name == "." || name == ".." {
        return None;
    }
    let attrs = data.dwFileAttributes;
    let is_dir = attrs & ATTR_DIRECTORY != 0;
    let ext = if is_dir {
        String::new()
    } else {
        name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
    };
    Some(Entry {
        size: ((data.nFileSizeHigh as u64) << 32) | data.nFileSizeLow as u64,
        modified: filetime(data.ftLastWriteTime),
        created: filetime(data.ftCreationTime),
        name,
        is_dir,
        ext,
        attrs,
        parent: None,
    })
}

/// Every entry of a folder (display path). `keep_going` is asked every few
/// thousand entries so a superseded load can stop early; it returns None then.
pub fn list_with(dir: &str, mut keep_going: impl FnMut() -> bool) -> Result<Option<Vec<Entry>>, String> {
    // A computer ("\\server") holds its shares, which no directory listing returns.
    if paths::kind(dir) == paths::Kind::Unc && paths::root(dir).is_none() {
        return crate::drives::shares(dir).map(Some);
    }
    let pattern = wide(&format!(r"{}\*", paths::to_native(dir).trim_end_matches('\\')));
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
    };
    let handle = match handle {
        Ok(h) => h,
        // An empty drive root has no "." entry, so "nothing found" means empty.
        Err(e) if e.code() == ERROR_FILE_NOT_FOUND.into() => return Ok(Some(Vec::new())),
        Err(e) => return Err(paths::open_error(dir, e.code().0 as u32 & 0xFFFF)),
    };
    let mut entries = Vec::with_capacity(256);
    let mut result = Ok(());
    loop {
        if let Some(e) = entry_from(&data) {
            entries.push(e);
            if entries.len() % 4096 == 0 && !keep_going() {
                unsafe { let _ = FindClose(handle); }
                return Ok(None);
            }
        }
        if let Err(e) = unsafe { FindNextFileW(handle, &mut data) } {
            if e.code() != ERROR_NO_MORE_FILES.into() {
                result = Err(paths::open_error(dir, e.code().0 as u32 & 0xFFFF));
            }
            break;
        }
    }
    unsafe { let _ = FindClose(handle); }
    result.map(|_| Some(entries))
}

pub fn list(dir: &str) -> Result<Vec<Entry>, String> {
    list_with(dir, || true).map(Option::unwrap_or_default)
}
