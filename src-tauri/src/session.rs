//! What a window keeps from the last run (F11): its size and place, and
//! whether it is the first Filezy window open, which is the one that brings
//! back the last tabs (the page keeps those).
//!
//! The place is Windows' own window placement, the normal bounds plus
//! whether it was maximized, saved when the window closes and applied before
//! it first shows. Windows moves a saved place that's now off every monitor
//! back onto one. It lives in %APPDATA%\Filezy beside the pins.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, SetWindowPlacement, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, SW_SHOWNORMAL, WINDOWPLACEMENT,
    WPF_RESTORETOMAXIMIZED,
};

const VERSION: u32 = 1;
/// How far a second window opens from the first, so it doesn't hide it exactly.
const CASCADE: i32 = 32;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Placement {
    v: u32,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    maximized: bool,
}

fn file() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var("APPDATA").ok()?).join("Filezy").join("window.json"))
}

static FIRST: AtomicBool = AtomicBool::new(true);

/// Note whether another Filezy window of this profile is open, by a named
/// mutex that lives as long as this process. Call once, before any window.
/// The name carries %APPDATA%, so a test profile never counts the user's own
/// windows, nor the other way round.
pub fn claim_first() {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    let profile = std::env::var("APPDATA").unwrap_or_default().to_lowercase();
    // FNV-1a: a short stable name; mutex names can't hold backslashes.
    let hash = profile.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let name = HSTRING::from(format!("Local\\Filezy-{hash:016x}"));
    match unsafe { CreateMutexW(None, false, &name) } {
        // The handle is never closed on purpose: the mutex goes when the process does.
        Ok(_) => FIRST.store(unsafe { GetLastError() } != ERROR_ALREADY_EXISTS, Ordering::SeqCst),
        Err(_) => FIRST.store(false, Ordering::SeqCst),
    }
}

/// True when no other Filezy window of this profile was open as this one started.
pub fn first() -> bool {
    FIRST.load(Ordering::SeqCst)
}

/// Remember where `hwnd` is, as it closes.
pub fn save(hwnd: isize) {
    let mut wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    if unsafe { GetWindowPlacement(HWND(hwnd as *mut _), &mut wp) }.is_err() {
        return;
    }
    let r = wp.rcNormalPosition;
    // Minimized from maximized comes back maximized, as Windows itself restores it.
    let maximized = wp.showCmd == SW_SHOWMAXIMIZED.0 as u32
        || (wp.showCmd == SW_SHOWMINIMIZED.0 as u32 && wp.flags.0 & WPF_RESTORETOMAXIMIZED.0 != 0);
    let p = Placement { v: VERSION, left: r.left, top: r.top, right: r.right, bottom: r.bottom, maximized };
    if let (Some(f), Ok(json)) = (file(), serde_json::to_string(&p)) {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, json);
    }
}

/// Show `hwnd` where the last window closed (a second window a little below
/// and to the right). False when nothing usable was saved; the caller shows
/// the window as configured.
pub fn restore(hwnd: isize) -> bool {
    let Some(p) = file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| serde_json::from_str::<Placement>(&s).ok()) else {
        return false;
    };
    if p.v != VERSION || p.right - p.left < 100 || p.bottom - p.top < 100 {
        return false;
    }
    let off = if first() { 0 } else { CASCADE };
    let wp = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        showCmd: if p.maximized { SW_SHOWMAXIMIZED.0 as u32 } else { SW_SHOWNORMAL.0 as u32 },
        ptMinPosition: POINT { x: -1, y: -1 },
        ptMaxPosition: POINT { x: -1, y: -1 },
        rcNormalPosition: RECT { left: p.left + off, top: p.top + off, right: p.right + off, bottom: p.bottom + off },
        ..Default::default()
    };
    unsafe { SetWindowPlacement(HWND(hwnd as *mut _), &wp) }.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_round_trips_and_rejects_other_versions() {
        let p = Placement { v: VERSION, left: 10, top: 20, right: 900, bottom: 700, maximized: true };
        let back: Placement = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        // A record from another version or a damaged one parses to nothing usable.
        assert!(serde_json::from_str::<Placement>(r#"{"v":1}"#).is_err());
    }
}
