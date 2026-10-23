//! Scripted drags for the E2E runner, in test sessions only (see `crate::e2e`).
//!
//! Windows' drag loop follows the physical mouse button and pointer, so a real
//! drag takes the developer's mouse. These drive the window's own drop target
//! the way that loop would, with the same shell data object, from a script of
//! points, rests and keys. Everything Filezy does is covered: the page's drop
//! destinations, the effect rules, the caption, the right-drag menu and the
//! drop. Only Windows' loop itself is not.

use std::cell::RefCell;
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

use crate::on_main_wait as on_main;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, HWND, POINT, POINTL};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{IDropTarget, ReleaseStgMedium, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE};
use windows::Win32::System::SystemServices::{MK_CONTROL, MK_LBUTTON, MK_RBUTTON, MK_SHIFT, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::{CFSTR_LOGICALPERFORMEDDROPEFFECT, CFSTR_PERFORMEDDROPEFFECT};

/// What every drag source here permits, as Explorer's does.
const ALLOWED: DROPEFFECT = DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_MOVE.0 | DROPEFFECT_LINK.0);

thread_local! {
    // UI thread only, where OLE calls the target: the window's drop target and
    // the data object of the drag in progress.
    static TARGET: RefCell<Option<IDropTarget>> = const { RefCell::new(None) };
    static DATA: RefCell<Option<IDataObject>> = const { RefCell::new(None) };
}

/// Called by `droptarget::install` in a test session.
pub fn set_target(target: &IDropTarget) {
    TARGET.with(|t| *t.borrow_mut() = Some(target.clone()));
}

/// A point in CSS pixels in the window, with how long the drag rests there
/// (default 60 ms) and the modifiers held from there on ("ctrl+shift", "none").
#[derive(Deserialize)]
pub struct Point {
    x: f64,
    y: f64,
    ms: Option<u64>,
    keys: Option<String>,
}

/// A drag: it enters at the first point and drops (or cancels) at the last.
#[derive(Deserialize)]
pub struct Script {
    points: Vec<Point>,
    /// Modifiers held from the start.
    #[serde(default)]
    keys: String,
    /// Dragged with the right button: the drop asks what to do.
    #[serde(default)]
    right: bool,
    #[serde(default)]
    cancel: bool,
}

/// What a drag source would see: hr (DoDragDrop's result), effect (what the
/// drop reported back), performed and logical (the Performed and Logical
/// Performed DropEffect formats the target set, -1 if none), and feedback
/// (each effect the cursor would have shown, in order).
#[derive(Serialize, Default)]
pub struct Outcome {
    hr: u32,
    effect: u32,
    performed: i64,
    logical: i64,
    feedback: Vec<u32>,
}

type Done = mpsc::Sender<Result<Outcome, String>>;

static ARMED: Mutex<Option<(Script, Done)>> = Mutex::new(None);
static PENDING: Mutex<Option<mpsc::Receiver<Result<Outcome, String>>>> = Mutex::new(None);

/// Script the next drag the page starts itself (`start_drag`).
pub fn arm(script: Script) {
    let (tx, rx) = mpsc::channel();
    *ARMED.lock().unwrap() = Some((script, tx));
    *PENDING.lock().unwrap() = Some(rx);
}

/// The armed script, for `start_drag` to run instead of a real drag.
pub fn take_armed() -> Option<(Script, Done)> {
    ARMED.lock().unwrap().take()
}

/// The outcome of the armed drag, once it has ended.
pub fn outcome() -> Result<Outcome, String> {
    let rx = PENDING.lock().unwrap().take().ok_or("no drag was armed")?;
    let out = rx.recv_timeout(Duration::from_secs(20));
    ARMED.lock().unwrap().take();
    out.map_err(|_| "the page never started the armed drag".to_string())?
}

fn target() -> Result<IDropTarget, String> {
    TARGET.with(|t| t.borrow().clone()).ok_or_else(|| "no drop target installed".to_string())
}

fn mods(keys: &str) -> u32 {
    keys.split(['+', ','])
        .map(|k| match k.trim() {
            "ctrl" => MK_CONTROL.0,
            "shift" => MK_SHIFT.0,
            // Windows' headers define MK_ALT, windows-rs doesn't.
            "alt" => 0x20,
            _ => 0,
        })
        .fold(0, |a, b| a | b)
}

/// A registered DWORD format the target set on the data object, or -1.
fn read_effect(data: &IDataObject, format: PCWSTR) -> i64 {
    let fmt = FORMATETC {
        cfFormat: unsafe { RegisterClipboardFormatW(format) } as u16,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    unsafe {
        let Ok(mut medium) = data.GetData(&fmt) else { return -1 };
        let p = GlobalLock(medium.u.hGlobal) as *const u32;
        let v = if p.is_null() { -1 } else { p.read_unaligned() as i64 };
        let _ = GlobalUnlock(medium.u.hGlobal);
        ReleaseStgMedium(&mut medium);
        v
    }
}

/// Drag `paths` through the window's drop target along the script.
pub fn run(app: &AppHandle, paths: Vec<String>, s: Script) -> Result<Outcome, String> {
    let win = app.get_webview_window("main").ok_or("no main window")?;
    let scale = win.scale_factor().map_err(|e| e.to_string())?;
    let top = win.hwnd().map_err(|e| e.to_string())?.0 as isize;
    let screen = |p: &Point| {
        let mut pt = POINT { x: (p.x * scale).round() as i32, y: (p.y * scale).round() as i32 };
        unsafe {
            let _ = ClientToScreen(HWND(top as *mut _), &mut pt);
        }
        POINTL { x: pt.x, y: pt.y }
    };
    let (first, last) = match (s.points.first(), s.points.last()) {
        (Some(f), Some(l)) => (screen(f), screen(l)),
        _ => return Err("a drag needs points".into()),
    };
    let button = if s.right { MK_RBUTTON.0 } else { MK_LBUTTON.0 };
    let mut keys = button | mods(&s.keys);
    let mut out = Outcome { performed: -1, logical: -1, ..Default::default() };
    let mut shown = |e: u32| {
        if out.feedback.last() != Some(&e) {
            out.feedback.push(e);
        }
    };

    let k = keys;
    shown(on_main(app, move || -> Result<u32, String> {
        let data = crate::shellmenu::data_object(&paths)?;
        DATA.with(|d| *d.borrow_mut() = Some(data.clone()));
        let mut e = ALLOWED;
        unsafe { target()?.DragEnter(&data, MODIFIERKEYS_FLAGS(k), first, &mut e) }.map_err(|e| e.message())?;
        Ok(e.0)
    })??);
    // The loop calls DragOver as the pointer moves and again while it rests,
    // which is when the page's answers (drop_dest) and its hover timers land.
    for p in &s.points {
        if let Some(k) = &p.keys {
            keys = button | mods(k);
        }
        let (at, k) = (screen(p), keys);
        let until = Instant::now() + Duration::from_millis(p.ms.unwrap_or(60));
        loop {
            shown(on_main(app, move || -> Result<u32, String> {
                let mut e = ALLOWED;
                unsafe { target()?.DragOver(MODIFIERKEYS_FLAGS(k), at, &mut e) }.map_err(|e| e.message())?;
                Ok(e.0)
            })??);
            if Instant::now() >= until {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    // The button is up by the time a drop happens.
    let (k, cancel) = (keys & !(MK_LBUTTON.0 | MK_RBUTTON.0), s.cancel);
    let (hr, effect, performed, logical) = on_main(app, move || -> Result<(u32, u32, i64, i64), String> {
        let data = DATA.with(|d| d.borrow_mut().take()).ok_or("no drag in progress")?;
        if cancel {
            unsafe { target()?.DragLeave() }.map_err(|e| e.message())?;
            return Ok((DRAGDROP_S_CANCEL.0 as u32, 0, -1, -1));
        }
        let mut e = ALLOWED;
        unsafe { target()?.Drop(&data, MODIFIERKEYS_FLAGS(k), last, &mut e) }.map_err(|e| e.message())?;
        Ok((
            DRAGDROP_S_DROP.0 as u32,
            e.0,
            read_effect(&data, CFSTR_PERFORMEDDROPEFFECT),
            read_effect(&data, CFSTR_LOGICALPERFORMEDDROPEFFECT),
        ))
    })??;
    Ok(Outcome { hr, effect, performed, logical, ..out })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_names() {
        assert_eq!(mods(""), 0);
        assert_eq!(mods("none"), 0);
        assert_eq!(mods("ctrl"), MK_CONTROL.0);
        assert_eq!(mods("ctrl+shift"), MK_CONTROL.0 | MK_SHIFT.0);
        assert_eq!(mods("ctrl,shift,alt"), MK_CONTROL.0 | MK_SHIFT.0 | 0x20);
    }
}
