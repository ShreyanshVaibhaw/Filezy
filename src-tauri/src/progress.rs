//! Windows' progress dialog, for long work Filezy does itself (extracting,
//! compressing). Like Explorer's, it appears only once the work has taken a
//! second, and its Cancel button is what the work checks.
use std::time::{Duration, Instant};
use windows::core::{GUID, HSTRING};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{IProgressDialog, PROGDLG_AUTOTIME, PROGDLG_NORMAL};

/// CLSID_ProgressDialog (shlguid.h); windows-rs doesn't name it.
const PROGRESS_DIALOG: GUID = GUID::from_u128(0xf8383852_fcd3_11d1_a6b9_006097df5bd4);

pub struct Progress {
    owner: HWND,
    title: String,
    line: String,
    started: Instant,
    last: Instant,
    dlg: Option<IProgressDialog>,
}

impl Progress {
    pub fn new(owner: isize, title: &str, line: &str) -> Self {
        let now = Instant::now();
        Progress { owner: HWND(owner as *mut _), title: title.into(), line: line.into(), started: now, last: now, dlg: None }
    }

    /// Report how far the work is; false once the user pressed Cancel.
    pub fn tick(&mut self, done: u64, total: u64, current: &str) -> bool {
        let now = Instant::now();
        // A few updates a second are plenty, and cost the work nothing.
        if now - self.last < Duration::from_millis(100) {
            return true;
        }
        self.last = now;
        if self.dlg.is_none() && now - self.started >= Duration::from_secs(1) {
            self.dlg = unsafe { CoCreateInstance::<_, IProgressDialog>(&PROGRESS_DIALOG, None, CLSCTX_INPROC_SERVER) }.ok();
            if let Some(d) = &self.dlg {
                unsafe {
                    let _ = d.SetTitle(&HSTRING::from(&self.title));
                    let _ = d.SetLine(1, &HSTRING::from(&self.line), false, None);
                    let _ = d.StartProgressDialog(Some(self.owner), None, PROGDLG_NORMAL | PROGDLG_AUTOTIME, None);
                }
            }
        }
        let Some(d) = &self.dlg else { return true };
        unsafe {
            let _ = d.SetProgress64(done, total.max(1));
            let _ = d.SetLine(2, &HSTRING::from(current), true, None);
            !d.HasUserCancelled().as_bool()
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        if let Some(d) = &self.dlg {
            unsafe { let _ = d.StopProgressDialog(); }
        }
    }
}

/// Run `f` on a fresh STA thread (the dialog is a COM object) and wait for it.
pub fn run<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    std::thread::spawn(move || {
        unsafe {
            if windows::Win32::System::Ole::OleInitialize(None).is_err() {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            }
        }
        let r = f();
        unsafe { windows::Win32::System::Ole::OleUninitialize() };
        r
    })
    .join()
    .map_err(|_| "The work stopped unexpectedly.".to_string())
}
