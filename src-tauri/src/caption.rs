//! Windows only offers Snap Layouts over a native HTMAXBUTTON hit target.
//! A small child window sits over the HTML maximize button and paints the same
//! icon; the rest of the title bar and all keyboard access remain in the page.
use serde::Deserialize;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreatePen, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetStockObject, LineTo, MoveToEx, Rectangle, SelectObject,
    HOLLOW_BRUSH, PAINTSTRUCT, PS_SOLID,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

static HANDLE: AtomicIsize = AtomicIsize::new(0);
static BG: AtomicU32 = AtomicU32::new(0x202020);
static FG: AtomicU32 = AtomicU32::new(0xffffff);

#[derive(Deserialize)]
pub struct Place {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub background: u32,
    pub foreground: u32,
    pub visible: bool,
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match message {
            WM_NCHITTEST => return LRESULT(HTMAXBUTTON as isize),
            WM_NCLBUTTONDOWN | WM_LBUTTONDOWN => return LRESULT(0),
            WM_NCLBUTTONUP | WM_LBUTTONUP => {
                if let Ok(parent) = GetParent(hwnd) {
                    let command = if IsZoomed(parent).as_bool() { SC_RESTORE } else { SC_MAXIMIZE };
                    let _ = PostMessageW(Some(parent), WM_SYSCOMMAND, WPARAM(command as usize), LPARAM(0));
                }
                return LRESULT(0);
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let bg = CreateSolidBrush(COLORREF(BG.load(Ordering::Relaxed)));
                FillRect(dc, &rect, bg);
                let _ = DeleteObject(bg.into());
                let size = ((rect.bottom - rect.top) as f64 * 0.3).round().max(8.0) as i32;
                let x = (rect.right - size) / 2;
                let y = (rect.bottom - size) / 2;
                let pen = CreatePen(PS_SOLID, 1, COLORREF(FG.load(Ordering::Relaxed)));
                let old_pen = SelectObject(dc, pen.into());
                let old_brush = SelectObject(dc, GetStockObject(HOLLOW_BRUSH));
                let max = GetParent(hwnd).is_ok_and(|p| IsZoomed(p).as_bool());
                if max {
                    let _ = Rectangle(dc, x, y + 2, x + size - 2, y + size);
                    let _ = MoveToEx(dc, x + 2, y + 2, None);
                    let _ = LineTo(dc, x + 2, y);
                    let _ = LineTo(dc, x + size, y);
                    let _ = LineTo(dc, x + size, y + size - 2);
                    let _ = LineTo(dc, x + size - 2, y + size - 2);
                } else {
                    let _ = Rectangle(dc, x, y, x + size, y + size);
                }
                SelectObject(dc, old_pen);
                SelectObject(dc, old_brush);
                let _ = DeleteObject(pen.into());
                let _ = EndPaint(hwnd, &paint);
                return LRESULT(0);
            }
            _ => {}
        }
        DefWindowProcW(hwnd, message, wp, lp)
    }
}

pub fn place(parent: isize, scale: f64, place: Place) -> Result<(), String> {
    if ![place.x, place.y, place.width, place.height, scale].iter().all(|n| n.is_finite())
        || !(0.5..=8.0).contains(&scale)
        || place.width < 1.0
        || place.width > 300.0
        || place.height < 1.0
        || place.height > 100.0
    {
        return Err("Invalid caption bounds.".into());
    }
    unsafe {
        let mut hwnd = HWND(HANDLE.load(Ordering::Relaxed) as *mut _);
        if hwnd.0.is_null() {
            let instance = HINSTANCE(GetModuleHandleW(None).map_err(|e| e.message())?.0);
            let class = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                lpszClassName: w!("FilezySnapCaption"),
                ..Default::default()
            };
            RegisterClassW(&class);
            hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE,
                w!("FilezySnapCaption"),
                w!("Maximize"),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                1,
                1,
                Some(HWND(parent as *mut _)),
                None,
                Some(instance),
                None,
            )
            .map_err(|e| e.message())?;
            HANDLE.store(hwnd.0 as isize, Ordering::Relaxed);
        }
        BG.store(place.background, Ordering::Relaxed);
        FG.store(place.foreground, Ordering::Relaxed);
        SetWindowPos(
            hwnd,
            Some(HWND_TOP),
            (place.x * scale).round() as i32,
            (place.y * scale).round() as i32,
            (place.width * scale).round() as i32,
            (place.height * scale).round() as i32,
            SWP_NOACTIVATE,
        )
        .map_err(|e| e.message())?;
        let _ = ShowWindow(hwnd, if place.visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, false);
    }
    Ok(())
}
