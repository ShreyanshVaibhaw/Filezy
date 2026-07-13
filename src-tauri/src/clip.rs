//! System clipboard interop via CF_HDROP so copy/paste round-trips with real Explorer.
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use windows::core::w;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

const CF_HDROP: u32 = 15;

#[repr(C)]
struct DropFiles {
    p_files: u32,
    pt: [i32; 2],
    f_nc: i32,
    f_wide: i32,
}

fn effect_format() -> u32 {
    unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) }
}

struct ClipGuard;
impl Drop for ClipGuard {
    fn drop(&mut self) {
        unsafe { let _ = CloseClipboard(); }
    }
}

fn alloc_global(bytes: &[u8]) -> Result<HGLOBAL, String> {
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|e| e.to_string())?;
        let ptr = GlobalLock(h) as *mut u8;
        if ptr.is_null() {
            return Err("GlobalLock failed".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(h);
        Ok(h)
    }
}

pub fn set_files(paths: &[PathBuf], cut: bool) -> Result<(), String> {
    unsafe {
        let mut wide: Vec<u16> = Vec::new();
        for p in paths {
            wide.extend(p.as_os_str().encode_wide());
            wide.push(0);
        }
        wide.push(0);

        let header = DropFiles {
            p_files: std::mem::size_of::<DropFiles>() as u32,
            pt: [0, 0],
            f_nc: 0,
            f_wide: 1,
        };
        let mut bytes = Vec::with_capacity(std::mem::size_of::<DropFiles>() + wide.len() * 2);
        bytes.extend_from_slice(std::slice::from_raw_parts(
            &header as *const _ as *const u8,
            std::mem::size_of::<DropFiles>(),
        ));
        bytes.extend_from_slice(std::slice::from_raw_parts(
            wide.as_ptr() as *const u8,
            wide.len() * 2,
        ));

        OpenClipboard(None).map_err(|e| e.to_string())?;
        let _guard = ClipGuard;
        EmptyClipboard().map_err(|e| e.to_string())?;

        // SetClipboardData only takes ownership of the HGLOBAL on success; free it if it fails.
        let h = alloc_global(&bytes)?;
        if let Err(e) = SetClipboardData(CF_HDROP, Some(HANDLE(h.0))) {
            let _ = GlobalFree(Some(h));
            return Err(e.to_string());
        }

        // DROPEFFECT_MOVE = 2, DROPEFFECT_COPY|DROPEFFECT_LINK = 5 (what Explorer sets)
        let effect: u32 = if cut { 2 } else { 5 };
        let h2 = alloc_global(&effect.to_le_bytes())?;
        if let Err(e) = SetClipboardData(effect_format(), Some(HANDLE(h2.0))) {
            let _ = GlobalFree(Some(h2));
            return Err(e.to_string());
        }
        Ok(())
    }
}

pub fn get_files() -> Option<(Vec<PathBuf>, bool)> {
    unsafe {
        OpenClipboard(None).ok()?;
        let _guard = ClipGuard;
        let h = GetClipboardData(CF_HDROP).ok()?;
        let hdrop = HDROP(h.0);
        let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
        let mut paths = Vec::with_capacity(count as usize);
        for i in 0..count {
            let mut buf = [0u16; 1024];
            let len = DragQueryFileW(hdrop, i, Some(&mut buf));
            if len > 0 {
                paths.push(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])));
            }
        }
        let mut cut = false;
        if let Ok(he) = GetClipboardData(effect_format()) {
            let ptr = GlobalLock(HGLOBAL(he.0)) as *const u32;
            if !ptr.is_null() {
                cut = *ptr & 2 != 0 && *ptr & 1 == 0;
                let _ = GlobalUnlock(HGLOBAL(he.0));
            }
        }
        if paths.is_empty() {
            None
        } else {
            Some((paths, cut))
        }
    }
}

pub fn set_text(text: &str) -> Result<(), String> {
    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes =
            std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2).to_vec();
        OpenClipboard(None).map_err(|e| e.to_string())?;
        let _guard = ClipGuard;
        EmptyClipboard().map_err(|e| e.to_string())?;
        let h = alloc_global(&bytes)?;
        SetClipboardData(CF_UNICODETEXT, Some(HANDLE(h.0))).map_err(|e| e.to_string())?;
        Ok(())
    }
}

pub fn has_files() -> bool {
    unsafe { IsClipboardFormatAvailable(CF_HDROP).is_ok() }
}

pub fn clear() {
    unsafe {
        if OpenClipboard(None).is_ok() {
            let _guard = ClipGuard;
            let _ = EmptyClipboard();
        }
    }
}
