//! Recycle Bin: enumerate, restore, delete and empty via the shell namespace.
//!
//! Items are identified by their parsing name in the bin (the "$R..." file),
//! never by display name: two deleted "report.txt" files are different items.
use serde::Serialize;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::Common::{ITEMIDLIST, STRRET};
use windows::Win32::UI::Shell::{
    IShellFolder, IShellFolder2, SHEmptyRecycleBinW, SHGetDesktopFolder, StrRetToStrW, SHCONTF_FOLDERS,
    SHCONTF_NONFOLDERS, SHGDN_FORPARSING, SHGDN_INFOLDER,
};

#[derive(Serialize)]
pub struct RecycleItem {
    /// Opaque identity: the item's parsing name inside the bin.
    pub id: String,
    pub name: String,
    pub original: String,
    pub deleted: String,
    pub size: String,
}

#[derive(Serialize)]
pub struct RecycleList {
    pub items: Vec<RecycleItem>,
    /// Items in the bin; more than `items.len()` when the list was capped.
    pub total: usize,
}

/// Rows the page shows at most; the rest are counted and reported.
const LIST_MAX: usize = 5000;

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
    crate::com::sta();
    unsafe {
        let desktop = SHGetDesktopFolder().map_err(|e| e.message().to_string())?;
        let w: Vec<u16> = BIN.encode_utf16().chain(std::iter::once(0)).collect();
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        desktop
            .ParseDisplayName(HWND::default(), None, PCWSTR(w.as_ptr()), None, &mut pidl, std::ptr::null_mut())
            .map_err(|e| e.message().to_string())?;
        let folder: Result<IShellFolder, _> = desktop.BindToObject(pidl, None);
        CoTaskMemFree(Some(pidl as *const _));
        folder.map_err(|e| e.message().to_string())
    }
}

fn name_of(folder: &IShellFolder, p: *const ITEMIDLIST, flags: windows::Win32::UI::Shell::SHGDNF) -> String {
    let mut sr = STRRET::default();
    if unsafe { folder.GetDisplayNameOf(p, flags, &mut sr) }.is_ok() {
        strret(&mut sr, p)
    } else {
        String::new()
    }
}

/// Visit every item in the bin with its id; the callback decides whether to
/// keep the pidl (kept pidls are returned, everything else is freed).
fn walk(folder: &IShellFolder, mut visit: impl FnMut(*const ITEMIDLIST, String) -> bool) -> Result<Vec<*const ITEMIDLIST>, String> {
    unsafe {
        let mut enumerator = None;
        folder
            .EnumObjects(HWND::default(), (SHCONTF_FOLDERS.0 | SHCONTF_NONFOLDERS.0) as u32, &mut enumerator)
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
            let id = name_of(folder, p, SHGDN_FORPARSING);
            if visit(p, id) {
                kept.push(p);
            } else {
                CoTaskMemFree(Some(pidl[0] as *const _));
            }
        }
        Ok(kept)
    }
}

/// List recycle bin contents. Columns: 1 original location, 2 date deleted, 3 size.
pub fn list() -> Result<RecycleList, String> {
    let folder = bin_folder()?;
    let f2: IShellFolder2 =
        windows::core::Interface::cast(&folder).map_err(|e: windows::core::Error| e.message().to_string())?;
    let mut items = Vec::new();
    let mut total = 0usize;
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
    walk(&folder, |p, id| {
        total += 1;
        if items.len() < LIST_MAX {
            items.push(RecycleItem {
                id,
                name: name_of(&folder, p, SHGDN_INFOLDER),
                original: detail(p, 1),
                deleted: detail(p, 2),
                size: detail(p, 3),
            });
        }
        false
    })?;
    Ok(RecycleList { items, total })
}

/// Invoke a bin verb ("undelete", "delete") on exactly the items with these ids.
/// Returns how many matched; ids no longer in the bin are an error.
fn invoke(ids: &[String], verb: &str, owner: isize) -> Result<usize, String> {
    use windows::Win32::UI::Shell::{IContextMenu, CMINVOKECOMMANDINFO};
    let folder = bin_folder()?;
    let wanted: std::collections::HashSet<String> = ids.iter().map(|s| s.to_lowercase()).collect();
    let pidls = walk(&folder, |_, id| wanted.contains(&id.to_lowercase()))?;
    let found = pidls.len();
    if found == 0 {
        return Err(if ids.len() == 1 {
            "That item is no longer in the Recycle Bin.".into()
        } else {
            "Those items are no longer in the Recycle Bin.".into()
        });
    }
    let res = unsafe {
        folder
            .GetUIObjectOf::<IContextMenu>(HWND::default(), &pidls, None)
            .map_err(|e| e.message().to_string())
            .and_then(|icm| {
                let verb = std::ffi::CString::new(verb).unwrap();
                let info = CMINVOKECOMMANDINFO {
                    cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
                    hwnd: HWND(owner as *mut _),
                    lpVerb: windows::core::PCSTR(verb.as_ptr() as *const u8),
                    ..Default::default()
                };
                icm.InvokeCommand(&info).map_err(|e| e.message().to_string())
            })
    };
    for p in &pidls {
        unsafe { CoTaskMemFree(Some(*p as *const _)) };
    }
    res?;
    if found < ids.len() {
        return Err(format!("{} of {} items were no longer in the Recycle Bin.", ids.len() - found, ids.len()));
    }
    Ok(found)
}

/// Put items back where they were deleted from.
pub fn restore(ids: &[String], owner: isize) -> Result<usize, String> {
    invoke(ids, "undelete", owner)
}

/// Delete items from the bin for good; Windows asks for confirmation first.
pub fn delete(ids: &[String], owner: isize) -> Result<usize, String> {
    invoke(ids, "delete", owner)
}

/// Native empty-bin flow: Windows shows its own confirmation and progress UI.
pub fn empty(owner: isize) -> Result<(), String> {
    crate::com::sta();
    unsafe { SHEmptyRecycleBinW(Some(HWND(owner as *mut _)), PCWSTR::null(), 0).map_err(|e| e.message().to_string()) }
}
