//! Real Explorer context menus via IContextMenu, rendered by our Fluent frontend.
//! COM objects live on a dedicated STA thread; build and invoke are paired calls.
use crate::fs_list::wide;
use serde::Serialize;
use std::sync::mpsc;
use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    IContextMenu, IShellFolder, SHBindToObject, SHParseDisplayName, CMF_NORMAL,
    CMINVOKECOMMANDINFO,
};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, DispatchMessageW, GetMenuItemCount, GetMenuItemInfoW,
    PeekMessageW, TranslateMessage, HMENU, MENUITEMINFOW, MFS_DISABLED, MFT_SEPARATOR,
    MIIM_FTYPE, MIIM_ID, MIIM_STATE, MIIM_STRING, MIIM_SUBMENU, MSG, PM_REMOVE, SW_SHOWNORMAL,
};

const ID_FIRST: u32 = 1;
const ID_LAST: u32 = 0x7FFF;

/// Free a batch of absolute PIDLs allocated by the shell (Parse/SHParseDisplayName).
fn free_pidls(pidls: &[*const ITEMIDLIST]) {
    for p in pidls {
        if !p.is_null() {
            unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(*p as *const _)) };
        }
    }
}

#[derive(Clone, Serialize)]
pub struct Item {
    pub id: u32,
    pub text: String,
    pub verb: Option<String>,
    pub sep: bool,
    pub disabled: bool,
    pub sub: Option<Vec<Item>>,
}

fn fill_verbs(icm: &IContextMenu, items: &mut [Item]) {
    for it in items {
        if !it.sep && it.id >= ID_FIRST {
            it.verb = verb_of(icm, it.id);
        }
        if let Some(sub) = &mut it.sub {
            fill_verbs(icm, sub);
        }
    }
}

pub enum FileOp {
    Paste { srcs: Vec<String>, dest: String, cut: bool },
    Delete { paths: Vec<String> },
}

enum Req {
    Build { dir: String, names: Vec<String>, reply: mpsc::Sender<Result<Vec<Item>, String>> },
    Invoke { id: u32, reply: mpsc::Sender<Result<(), String>> },
    Properties { path: String, reply: mpsc::Sender<Result<(), String>> },
    Drag { paths: Vec<String>, reply: mpsc::Sender<Result<(), String>> },
    FileOp { op: FileOp, reply: mpsc::Sender<Result<(), String>> },
}

#[derive(Clone)]
pub struct ShellMenu {
    tx: mpsc::Sender<Req>,
}

impl Default for ShellMenu {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel::<Req>();
        std::thread::spawn(move || {
            unsafe {
                // OleInitialize (not just CoInitializeEx): DoDragDrop and OLE clipboard need it.
                if windows::Win32::System::Ole::OleInitialize(None).is_err() {
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                }
            }
            let mut current: Option<(IContextMenu, HMENU)> = None;
            loop {
                // STA threads must pump messages or OLE (clipboard verbs, COM marshaling) fails.
                unsafe {
                    let mut msg = MSG::default();
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                let req = match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                    Ok(r) => r,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                match req {
                    Req::Build { dir, names, reply } => {
                        if let Some((_, hmenu)) = current.take() {
                            unsafe { let _ = DestroyMenu(hmenu); }
                        }
                        let res = build(&dir, &names);
                        let _ = match res {
                            Ok((icm, hmenu, items)) => {
                                current = Some((icm, hmenu));
                                reply.send(Ok(items))
                            }
                            Err(e) => reply.send(Err(e)),
                        };
                    }
                    Req::Invoke { id, reply } => {
                        let res = match &current {
                            Some((icm, _)) => {
                                // Canonical verb first (survives shell-extension hooks), offset fallback.
                                match verb_of(icm, id) {
                                    Some(verb) => invoke_verb(icm, &verb).or_else(|_| invoke_offset(icm, id)),
                                    None => invoke_offset(icm, id),
                                }
                            }
                            None => Err("no active menu".into()),
                        };
                        let _ = reply.send(res);
                    }
                    Req::Properties { path, reply } => {
                        let _ = reply.send(show_properties(&path));
                    }
                    Req::Drag { paths, reply } => {
                        let _ = reply.send(do_drag(&paths));
                    }
                    Req::FileOp { op, reply } => {
                        let _ = reply.send(file_op(&op));
                    }
                }
            }
        });
        Self { tx }
    }
}

impl ShellMenu {
    pub fn build(&self, dir: String, names: Vec<String>) -> Result<Vec<Item>, String> {
        let (rtx, rrx) = mpsc::channel();
        self.tx.send(Req::Build { dir, names, reply: rtx }).map_err(|e| e.to_string())?;
        rrx.recv().map_err(|e| e.to_string())?
    }
    pub fn invoke(&self, id: u32) -> Result<(), String> {
        let (rtx, rrx) = mpsc::channel();
        self.tx.send(Req::Invoke { id, reply: rtx }).map_err(|e| e.to_string())?;
        rrx.recv().map_err(|e| e.to_string())?
    }
    pub fn properties(&self, path: String) -> Result<(), String> {
        let (rtx, rrx) = mpsc::channel();
        self.tx.send(Req::Properties { path, reply: rtx }).map_err(|e| e.to_string())?;
        rrx.recv().map_err(|e| e.to_string())?
    }
    pub fn drag(&self, paths: Vec<String>) -> Result<(), String> {
        let (rtx, rrx) = mpsc::channel();
        self.tx.send(Req::Drag { paths, reply: rtx }).map_err(|e| e.to_string())?;
        rrx.recv().map_err(|e| e.to_string())?
    }
    pub fn file_op(&self, op: FileOp) -> Result<(), String> {
        let (rtx, rrx) = mpsc::channel();
        self.tx.send(Req::FileOp { op, reply: rtx }).map_err(|e| e.to_string())?;
        rrx.recv().map_err(|e| e.to_string())?
    }
}

/// Explorer's own copy engine: native progress dialog, conflict dialogs
/// (Replace/Skip/Compare), recycle-bin delete, and undo registration.
/// Must run on the pumped STA thread (the dialogs are COM UI).
/// ponytail: one op at a time on this thread; a long paste queues later menu builds behind it
fn file_op(op: &FileOp) -> Result<(), String> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{
        FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName,
        FILEOPERATION_FLAGS,
    };
    const FOF_ALLOWUNDO: FILEOPERATION_FLAGS = FILEOPERATION_FLAGS(0x40);
    const FOF_RENAMEONCOLLISION: FILEOPERATION_FLAGS = FILEOPERATION_FLAGS(0x8);
    const FOFX_ADDUNDORECORD: FILEOPERATION_FLAGS = FILEOPERATION_FLAGS(0x2000_0000);

    fn item(path: &str) -> Result<IShellItem, String> {
        let w = wide(path);
        unsafe {
            SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None)
                .map_err(|e| e.message().to_string())
        }
    }

    unsafe {
        let fo: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| e.message().to_string())?;
        match op {
            FileOp::Paste { srcs, dest, cut } => {
                let dest_item = item(dest)?;
                // Paste back into the source folder copies with Explorer's " - Copy" naming.
                let same_dir = srcs.iter().all(|s| {
                    std::path::Path::new(s).parent()
                        == Some(std::path::Path::new(dest.trim_end_matches('\\')))
                });
                let mut flags = FOF_ALLOWUNDO | FOFX_ADDUNDORECORD;
                if !*cut && same_dir {
                    flags |= FOF_RENAMEONCOLLISION;
                }
                fo.SetOperationFlags(flags).map_err(|e| e.message().to_string())?;
                for s in srcs {
                    let it = item(s)?;
                    if *cut {
                        fo.MoveItem(&it, &dest_item, PCWSTR::null(), None)
                    } else {
                        fo.CopyItem(&it, &dest_item, PCWSTR::null(), None)
                    }
                    .map_err(|e| e.message().to_string())?;
                }
            }
            FileOp::Delete { paths } => {
                fo.SetOperationFlags(FOF_ALLOWUNDO).map_err(|e| e.message().to_string())?;
                for p in paths {
                    fo.DeleteItem(&item(p)?, None).map_err(|e| e.message().to_string())?;
                }
            }
        }
        // COPYENGINE_E_USER_CANCELLED: Cancel in the progress dialog is not an error.
        const USER_CANCELLED: i32 = 0x8027_0000u32 as i32;
        match fo.PerformOperations() {
            Ok(()) => Ok(()),
            Err(e) if e.code().0 == USER_CANCELLED => Ok(()),
            Err(e) => Err(format!("{} ({:#x})", e.message(), e.code().0)),
        }
    }
}

#[windows::core::implement(windows::Win32::System::Ole::IDropSource)]
struct DropSource;

impl windows::Win32::System::Ole::IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(
        &self,
        fescapepressed: windows::core::BOOL,
        grfkeystate: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
    ) -> windows::core::HRESULT {
        use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, S_OK};
        use windows::Win32::System::SystemServices::MK_LBUTTON;
        if fescapepressed.as_bool() {
            DRAGDROP_S_CANCEL
        } else if grfkeystate.0 & MK_LBUTTON.0 == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }
    fn GiveFeedback(
        &self,
        _dweffect: windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::HRESULT {
        windows::Win32::Foundation::DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Start an OLE drag with a shell-provided data object (CF_HDROP etc. all included),
/// so Explorer and any other app accept the drop. Blocks until the drag ends.
fn do_drag(paths: &[String]) -> Result<(), String> {
    use windows::Win32::System::Ole::{
        DoDragDrop, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE,
    };
    use windows::Win32::UI::Shell::{
        IShellItemArray, SHCreateShellItemArrayFromIDLists, BHID_DataObject,
    };
    unsafe {
        let mut pidls: Vec<*const ITEMIDLIST> = Vec::with_capacity(paths.len());
        for p in paths {
            let wp = wide(p);
            let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
            if let Err(e) = SHParseDisplayName(PCWSTR(wp.as_ptr()), None, &mut pidl, 0, None) {
                free_pidls(&pidls); // free the ones parsed so far before bailing
                return Err(e.message().to_string());
            }
            pidls.push(pidl);
        }
        let arr: Result<IShellItemArray, _> = SHCreateShellItemArrayFromIDLists(&pidls);
        free_pidls(&pidls);
        let arr = arr.map_err(|e| e.message().to_string())?;
        let dobj: windows::Win32::System::Com::IDataObject =
            arr.BindToHandler(None, &BHID_DataObject).map_err(|e| e.message().to_string())?;
        let src: windows::Win32::System::Ole::IDropSource = DropSource.into();
        let mut effect = DROPEFFECT::default();
        // Returns DRAGDROP_S_DROP or DRAGDROP_S_CANCEL; both are fine outcomes.
        let _ = DoDragDrop(
            &dobj,
            &src,
            DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK,
            &mut effect,
        );
        Ok(())
    }
}

/// Real shell Properties dialog. Must run on the pumped STA thread (modeless sheet).
fn show_properties(path: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW};
    unsafe {
        let wpath = wide(path);
        let verb = wide("properties");
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_INVOKEIDLIST,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(wpath.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        ShellExecuteExW(&mut sei).map_err(|e| e.message().to_string())
    }
}

fn build(dir: &str, names: &[String]) -> Result<(IContextMenu, HMENU, Vec<Item>), String> {
    // dir == "": names[0] is an absolute path with no listing folder (drive tiles
    // on This PC) - bind it to its parent folder via the full pidl.
    if dir.is_empty() {
        let path = names.first().ok_or("no path")?;
        unsafe {
            let w = wide(path);
            let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
            SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut pidl, 0, None)
                .map_err(|e| e.message().to_string())?;
            let mut child: *mut ITEMIDLIST = std::ptr::null_mut();
            let parent: windows::core::Result<IShellFolder> =
                windows::Win32::UI::Shell::SHBindToParent(pidl, Some(&raw mut child));
            let icm = parent.and_then(|f| {
                f.GetUIObjectOf::<IContextMenu>(HWND::default(), &[child as *const ITEMIDLIST], None)
            });
            // `child` points into `pidl`; free only after GetUIObjectOf.
            windows::Win32::System::Com::CoTaskMemFree(Some(pidl as *const _));
            let icm = icm.map_err(|e| e.message().to_string())?;
            let hmenu = CreatePopupMenu().map_err(|e| e.to_string())?;
            if let Err(e) = icm.QueryContextMenu(hmenu, 0, ID_FIRST, ID_LAST, CMF_NORMAL).ok() {
                let _ = DestroyMenu(hmenu);
                return Err(e.message().to_string());
            }
            let mut items = read_menu(hmenu, 0);
            fill_verbs(&icm, &mut items);
            return Ok((icm, hmenu, items));
        }
    }
    unsafe {
        let wdir = wide(dir);
        let mut dir_pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        SHParseDisplayName(PCWSTR(wdir.as_ptr()), None, &mut dir_pidl, 0, None)
            .map_err(|e| e.message().to_string())?;

        let bind_res: windows::core::Result<IShellFolder> =
            SHBindToObject(None, dir_pidl, None);
        windows::Win32::System::Com::CoTaskMemFree(Some(dir_pidl as *const _));
        let folder = bind_res.map_err(|e| e.message().to_string())?;

        let icm: IContextMenu = if names.is_empty() {
            // Folder background menu (New, Paste, Refresh, ...)
            folder
                .CreateViewObject::<IContextMenu>(HWND::default())
                .map_err(|e| e.message().to_string())?
        } else {
            let mut pidls: Vec<*const ITEMIDLIST> = Vec::with_capacity(names.len());
            for name in names {
                let wname = wide(name);
                let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
                if let Err(e) = folder.ParseDisplayName(
                    HWND::default(),
                    None,
                    PCWSTR(wname.as_ptr()),
                    None,
                    &mut pidl,
                    std::ptr::null_mut(),
                ) {
                    free_pidls(&pidls); // free the ones parsed so far before bailing
                    return Err(e.message().to_string());
                }
                pidls.push(pidl);
            }
            let res = folder.GetUIObjectOf::<IContextMenu>(HWND::default(), &pidls, None);
            free_pidls(&pidls);
            res.map_err(|e| e.message().to_string())?
        };

        let hmenu = CreatePopupMenu().map_err(|e| e.to_string())?;
        if let Err(e) = icm.QueryContextMenu(hmenu, 0, ID_FIRST, ID_LAST, CMF_NORMAL).ok() {
            let _ = DestroyMenu(hmenu); // don't leak the popup on failure
            return Err(e.message().to_string());
        }
        let mut items = read_menu(hmenu, 0);
        fill_verbs(&icm, &mut items);
        Ok((icm, hmenu, items))
    }
}

fn read_menu(hmenu: HMENU, depth: u8) -> Vec<Item> {
    let mut out = Vec::new();
    unsafe {
        let count = GetMenuItemCount(Some(hmenu));
        for i in 0..count.max(0) {
            let mut buf = [0u16; 256];
            let mut mii = MENUITEMINFOW {
                cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_ID | MIIM_STRING | MIIM_SUBMENU | MIIM_FTYPE | MIIM_STATE,
                dwTypeData: windows::core::PWSTR(buf.as_mut_ptr()),
                cch: 255,
                ..Default::default()
            };
            if GetMenuItemInfoW(hmenu, i as u32, true, &mut mii).is_err() {
                continue;
            }
            let sep = mii.fType.0 & MFT_SEPARATOR.0 != 0;
            let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
            let text = String::from_utf16_lossy(&buf[..len]).replace('&', "");
            let sub = if !mii.hSubMenu.is_invalid() && depth < 1 {
                // ponytail: lazily-populated submenus (Open with, Send to) are read as-is;
                // WM_INITMENUPOPUP forwarding would fill the stragglers
                let s = read_menu(mii.hSubMenu, depth + 1);
                if s.is_empty() { None } else { Some(s) }
            } else {
                None
            };
            if sep {
                out.push(Item {
                    id: 0,
                    text: String::new(),
                    verb: None,
                    sep: true,
                    disabled: false,
                    sub: None,
                });
            } else if !text.is_empty() {
                out.push(Item {
                    id: mii.wID,
                    text,
                    verb: None,
                    sep: false,
                    disabled: mii.fState.0 & MFS_DISABLED.0 != 0,
                    sub,
                });
            }
        }
    }
    // Collapse doubled separators left by filtered-out ownerdraw items.
    out.dedup_by(|a, b| a.sep && b.sep);
    while out.first().is_some_and(|i| i.sep) {
        out.remove(0);
    }
    while out.last().is_some_and(|i| i.sep) {
        out.pop();
    }
    out
}

fn verb_of(icm: &IContextMenu, id: u32) -> Option<String> {
    unsafe {
        let mut buf = [0u16; 256];
        // GCS_VERBW = 4: buffer receives wide chars despite the PSTR type.
        icm.GetCommandString(
            (id - ID_FIRST) as usize,
            4,
            None,
            windows::core::PSTR(buf.as_mut_ptr() as *mut u8),
            255,
        )
        .ok()?;
        let len = buf.iter().position(|&c| c == 0)?;
        if len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

fn invoke_verb(icm: &IContextMenu, verb: &str) -> Result<(), String> {
    unsafe {
        let cverb = std::ffi::CString::new(verb).map_err(|e| e.to_string())?;
        let info = CMINVOKECOMMANDINFO {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
            lpVerb: PCSTR(cverb.as_ptr() as *const u8),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        icm.InvokeCommand(&info).map_err(|e| e.message().to_string())
    }
}

fn invoke_offset(icm: &IContextMenu, id: u32) -> Result<(), String> {
    unsafe {
        let info = CMINVOKECOMMANDINFO {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
            lpVerb: PCSTR((id - ID_FIRST) as usize as *const u8),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        icm.InvokeCommand(&info).map_err(|e| e.message().to_string())
    }
}
