//! System clipboard interop via CF_HDROP so copy/paste round-trips with real Explorer.
use windows::core::w;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
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

/// Open the clipboard, waiting briefly while another app holds it open (they
/// do, for a moment, every time they write it).
fn open() -> Result<ClipGuard, String> {
    for i in 0.. {
        match unsafe { OpenClipboard(None) } {
            Ok(()) => return Ok(ClipGuard),
            Err(e) if i >= 10 => return Err(e.to_string()),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(15)),
        }
    }
    unreachable!()
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

/// Put display paths on the clipboard the way Explorer does, so its Paste works.
pub fn set_files(paths: &[String], cut: bool) -> Result<(), String> {
    unsafe {
        let mut wide: Vec<u16> = Vec::new();
        for p in paths {
            wide.extend(p.encode_utf16());
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

        let _guard = open()?;
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

/// The clipboard's files and whether they were cut.
pub fn get_files() -> Option<(Vec<String>, bool)> {
    unsafe {
        let _guard = open().ok()?;
        let h = GetClipboardData(CF_HDROP).ok()?;
        let paths = hdrop_paths(HDROP(h.0));
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

/// The paths in a CF_HDROP block, as display paths.
pub fn hdrop_paths(hdrop: HDROP) -> Vec<String> {
    unsafe {
        let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
        let mut paths = Vec::with_capacity(count as usize);
        for i in 0..count {
            // Ask for the length first: paths can be far longer than MAX_PATH.
            let need = DragQueryFileW(hdrop, i, None) as usize;
            let mut buf = vec![0u16; need + 1];
            let len = DragQueryFileW(hdrop, i, Some(&mut buf));
            if len > 0 {
                paths.push(crate::paths::to_display(&String::from_utf16_lossy(&buf[..len as usize])));
            }
        }
        paths
    }
}

pub fn set_text(text: &str) -> Result<(), String> {
    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes =
            std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2).to_vec();
        let _guard = open()?;
        EmptyClipboard().map_err(|e| e.to_string())?;
        let h = alloc_global(&bytes)?;
        SetClipboardData(CF_UNICODETEXT, Some(HANDLE(h.0))).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Changes whenever anything writes the clipboard, so a paste can tell whether
/// the clipboard still holds what it read.
pub fn sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

pub fn has_files() -> bool {
    unsafe { IsClipboardFormatAvailable(CF_HDROP).is_ok() }
}

static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

/// Emit "clipboard-changed" whenever any app writes the clipboard, so Paste and
/// the cut look follow other apps' copies at once. Windows tells a listener
/// window; a message-only window on its own thread is enough.
pub fn watch(app: tauri::AppHandle) {
    use tauri::Emitter;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::DataExchange::AddClipboardFormatListener;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, HWND_MESSAGE, MSG,
        WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WNDCLASSW,
    };

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_CLIPBOARDUPDATE {
            if let Some(app) = APP.get() {
                let _ = app.emit("clipboard-changed", ());
            }
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    if APP.set(app).is_err() {
        return;
    }
    std::thread::spawn(|| unsafe {
        let class = w!("FilezyClipboardListener");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE::default(), class, w!(""), WINDOW_STYLE::default(), 0, 0, 0, 0,
            Some(HWND_MESSAGE), None, None, None,
        ) else {
            return;
        };
        if AddClipboardFormatListener(hwnd).is_err() {
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    });
}

pub fn clear() {
    unsafe {
        if OpenClipboard(None).is_ok() {
            let _guard = ClipGuard;
            let _ = EmptyClipboard();
        }
    }
}
