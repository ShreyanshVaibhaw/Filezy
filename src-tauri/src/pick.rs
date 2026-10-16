//! Windows' folder picker.
use crate::fs_list::wide;
use crate::paths;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
    SIGDN_FILESYSPATH,
};

/// Ask for a folder, starting in `start`. None when the user cancels. Runs on
/// the UI thread (the dialog is modal to `owner`).
pub fn folder(owner: isize, start: &str, title: &str) -> Result<Option<String>, String> {
    let err = |e: windows::core::Error| e.message().to_string();
    unsafe {
        let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).map_err(err)?;
        let opts = d.GetOptions().map_err(err)?;
        d.SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST).map_err(err)?;
        let _ = d.SetTitle(&HSTRING::from(title));
        let w = wide(&paths::to_native(start));
        if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(w.as_ptr()), None) {
            let _ = d.SetFolder(&item);
        }
        if let Err(e) = d.Show(Some(HWND(owner as *mut _))) {
            return if e.code() == ERROR_CANCELLED.to_hresult() { Ok(None) } else { Err(err(e)) };
        }
        let p = d.GetResult().and_then(|item| item.GetDisplayName(SIGDN_FILESYSPATH)).map_err(err)?;
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        Ok(Some(paths::to_display(&s)))
    }
}
