//! Recycle Bin: enumerate, restore, and empty via the shell namespace.
use serde::Serialize;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::Common::{ITEMIDLIST, STRRET};
use windows::Win32::UI::Shell::{
    IShellFolder, IShellFolder2, SHEmptyRecycleBinW, SHGetDesktopFolder, StrRetToStrW,
    SHCONTF_FOLDERS, SHCONTF_NONFOLDERS, SHGDN_INFOLDER,
};

#[derive(Serialize)]
pub struct RecycleItem {
    pub name: String,
    pub original: String,
    pub deleted: String,
    pub size: String,
}

thread_local! {
    static COM_INIT: () = unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); };
}

const BIN: &str = "::{645FF040-5081-101B-9F08-00AA002F954E}";

fn strret(sr: &mut STRRET, pidl: *const ITEMIDLIST) -> String {
    unsafe {
        let mut p = windows::core::PWSTR::null();
        if StrRetToStrW(sr, Some(pidl), &mut p).is_err() || p.is_null() {
            return String::new();
        }
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

fn bin_folder() -> Result<IShellFolder, String> {
    COM_INIT.with(|_| {});
    unsafe {
        let desktop = SHGetDesktopFolder().map_err(|e| e.message().to_string())?;
        let w: Vec<u16> = BIN.encode_utf16().chain(std::iter::once(0)).collect();
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        desktop
            .ParseDisplayName(
                HWND::default(),
                None,
                PCWSTR(w.as_ptr()),
                None,
                &mut pidl,
                std::ptr::null_mut(),
            )
            .map_err(|e| e.message().to_string())?;
        let folder: Result<IShellFolder, _> = desktop.BindToObject(pidl, None);
        CoTaskMemFree(Some(pidl as *const _));
        folder.map_err(|e| e.message().to_string())
    }
}

/// Visit every item in the bin; the callback decides whether to keep the pidl
/// (kept pidls are returned, everything else is freed).
fn walk(
    folder: &IShellFolder,
    mut visit: impl FnMut(*const ITEMIDLIST, String) -> bool,
) -> Result<Vec<*const ITEMIDLIST>, String> {
    unsafe {
        let mut enumerator = None;
        folder
            .EnumObjects(
                HWND::default(),
                (SHCONTF_FOLDERS.0 | SHCONTF_NONFOLDERS.0) as u32,
                &mut enumerator,
            )
            .ok()
            .map_err(|e| e.message().to_string())?;
        let enumerator = enumerator.ok_or("no enumerator")?;
        let mut kept = Vec::new();
        let mut pidl: [*mut ITEMIDLIST; 1] = [std::ptr::null_mut()];
        let mut fetched = 0u32;
        loop {
            if enumerator.Next(&mut pidl, Some(&mut fetched)).is_err() || fetched != 1 {
                break;
            }
            let p = pidl[0] as *const ITEMIDLIST;
            let mut sr = STRRET::default();
            let name = if folder.GetDisplayNameOf(p, SHGDN_INFOLDER, &mut sr).is_ok() {
                strret(&mut sr, p)
            } else {
                String::new()
            };
            if visit(p, name) {
                kept.push(p);
            } else {
                CoTaskMemFree(Some(pidl[0] as *const _));
            }
        }
        Ok(kept)
    }
}

/// List recycle bin contents. Columns: 0 name, 1 original location, 2 date deleted, 3 size.
pub fn list() -> Result<Vec<RecycleItem>, String> {
    let folder = bin_folder()?;
    let f2: IShellFolder2 = windows::core::Interface::cast(&folder)
        .map_err(|e: windows::core::Error| e.message().to_string())?;
    let mut out = Vec::new();
    let detail = |pidl: *const ITEMIDLIST, col: u32| -> String {
        unsafe {
            let mut d = windows::Win32::UI::Shell::Common::SHELLDETAILS::default();
            if f2.GetDetailsOf(pidl, col, &mut d).is_ok() {
                // SHELLDETAILS is packed: copy the STRRET out before taking a reference.
                let mut sr = std::ptr::addr_of!(d.str).read_unaligned();
                strret(&mut sr, pidl)
            } else {
                String::new()
            }
        }
    };
    walk(&folder, |p, name| {
        if out.len() < 5000 {
            out.push(RecycleItem {
                name,
                original: detail(p, 1),
                deleted: detail(p, 2),
                size: detail(p, 3),
            });
        }
        false
    })?;
    Ok(out)
}

/// Restore items by their in-folder display names via the "undelete" verb.
pub fn restore(names: &[String]) -> Result<(), String> {
    use windows::Win32::UI::Shell::{IContextMenu, CMINVOKECOMMANDINFO};
    let folder = bin_folder()?;
    let pidls = walk(&folder, |_, name| names.iter().any(|n| n == &name))?;
    if pidls.is_empty() {
        return Ok(());
    }
    unsafe {
        let res = folder
            .GetUIObjectOf::<IContextMenu>(HWND::default(), &pidls, None)
            .map_err(|e| e.message().to_string())
            .and_then(|icm| {
                let verb = std::ffi::CString::new("undelete").unwrap();
                let info = CMINVOKECOMMANDINFO {
                    cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
                    lpVerb: windows::core::PCSTR(verb.as_ptr() as *const u8),
                    ..Default::default()
                };
                icm.InvokeCommand(&info).map_err(|e| e.message().to_string())
            });
        for p in &pidls {
            CoTaskMemFree(Some(*p as *const _));
        }
        res
    }
}

/// Native empty-bin flow: Windows shows its own confirmation and progress UI.
pub fn empty() -> Result<(), String> {
    COM_INIT.with(|_| {});
    unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), 0).map_err(|e| e.message().to_string()) }
}
