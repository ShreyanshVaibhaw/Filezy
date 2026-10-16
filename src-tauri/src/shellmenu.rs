//! Real Explorer context menus via IContextMenu.
//!
//! Two ways to show one: the Fluent menu (the page renders the items this
//! module reads out, built on a dedicated STA worker because extensions can
//! take seconds to load), and Windows' own menu ("Show more options"), tracked
//! on the UI thread with the menu messages extensions need forwarded to them.
//! Both are built from full paths the backend resolved, never from what the
//! page happened to have fetched.
use crate::fs_list::wide;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use windows::core::{Interface, PCSTR, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    IContextMenu, IContextMenu2, IContextMenu3, IShellFolder, ILClone, ILFindLastID, ILFree, ILIsEqual, ILRemoveLastID,
    SHBindToParent, SHGetDesktopFolder, SHParseDisplayName, CMF_CANRENAME, CMF_EXTENDEDVERBS, CMF_NORMAL, CMINVOKECOMMANDINFO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, DispatchMessageW, GetMenuItemCount, GetMenuItemInfoW, PeekMessageW,
    TranslateMessage, HMENU, MENUITEMINFOW, MFS_DISABLED, MFT_SEPARATOR, MIIM_FTYPE, MIIM_ID, MIIM_STATE,
    MIIM_STRING, MIIM_SUBMENU, MSG, PM_REMOVE, SW_SHOWNORMAL, WM_INITMENUPOPUP,
};

const ID_FIRST: u32 = 1;
const ID_LAST: u32 = 0x7FFF;
/// Beyond this many items the shell's menu isn't built: extensions can do work
/// per item, and a menu that takes minutes is worse than an explained absence.
const MENU_MAX: usize = 10_000;

/// MENU_MAX, or a smaller cap for the E2E tests (FILEZY_E2E_MENU_MAX).
pub fn menu_max() -> usize {
    static MAX: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *MAX.get_or_init(|| crate::e2e_knob("MENU_MAX").unwrap_or(MENU_MAX))
}
/// Submenu levels read out for the Fluent menu.
const DEPTH: u8 = 3;

#[derive(Clone, Serialize)]
pub struct Item {
    pub id: u32,
    pub text: String,
    pub verb: Option<String>,
    pub sep: bool,
    pub disabled: bool,
    pub sub: Option<Vec<Item>>,
}

/// A built menu. `session` names it: invoking takes it back, and a command
/// from any other menu is refused.
#[derive(Clone, Serialize)]
pub struct Menu {
    pub session: u64,
    pub items: Vec<Item>,
    /// Why there are no shell entries, when there aren't any.
    pub note: Option<String>,
}

static SESSION: AtomicU64 = AtomicU64::new(1);

/// Absolute item IDs, freed together.
struct Pidls(Vec<*mut ITEMIDLIST>);

impl Drop for Pidls {
    fn drop(&mut self) {
        for p in &self.0 {
            unsafe { ILFree(Some(*p)) };
        }
    }
}

fn parse(paths: &[String]) -> Result<Pidls, String> {
    let mut out = Pidls(Vec::with_capacity(paths.len()));
    for p in paths {
        let w = wide(p);
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        unsafe { SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut pidl, 0, None) }
            .map_err(|e| format!("{}: {}", crate::paths::file_name(p), e.message()))?;
        out.0.push(pidl);
    }
    Ok(out)
}

/// The shell's context menu for these items. Items in one folder go through
/// that folder, as in Explorer; items from several folders (search results)
/// go through the desktop folder with absolute IDs.
fn context_menu(paths: &[String]) -> Result<IContextMenu, String> {
    let pidls = parse(paths)?;
    let first = *pidls.0.first().ok_or("Nothing is selected.")?;
    unsafe {
        let parent_of = |p: *const ITEMIDLIST| {
            let c = ILClone(p);
            let _ = ILRemoveLastID(Some(c));
            c
        };
        let parent = parent_of(first);
        let same = pidls.0.iter().all(|p| {
            let q = parent_of(*p);
            let eq = ILIsEqual(parent, q).as_bool();
            ILFree(Some(q));
            eq
        });
        ILFree(Some(parent));
        if same {
            let folder: IShellFolder = SHBindToParent(first, None).map_err(|e| e.message().to_string())?;
            let children: Vec<*const ITEMIDLIST> = pidls.0.iter().map(|p| ILFindLastID(*p) as *const _).collect();
            folder.GetUIObjectOf::<IContextMenu>(HWND::default(), &children, None).map_err(|e| e.message().to_string())
        } else {
            several_folders(paths, &pidls)
        }
    }
}

/// The shell's default menu for items in several folders (search results). The
/// desktop folder alone knows nothing of their types, so it is given the
/// registry keys Explorer uses for files and folders: Send to, Open with and
/// the extensions registered there come along.
fn several_folders(paths: &[String], pidls: &Pidls) -> Result<IContextMenu, String> {
    use std::mem::ManuallyDrop;
    use windows::core::w;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, HKEY, HKEY_CLASSES_ROOT, KEY_READ};
    use windows::Win32::UI::Shell::{SHCreateDefaultContextMenu, DEFCONTEXTMENU};
    const DIRECTORY: u32 = 0x10;
    let folders = paths.iter().any(|p| crate::paths::attributes(p).is_ok_and(|a| a & DIRECTORY != 0));
    let mut names = vec![w!("*"), w!("AllFilesystemObjects")];
    if folders {
        names.extend([w!("Directory"), w!("Folder")]);
    }
    unsafe {
        let keys: Vec<HKEY> = names
            .into_iter()
            .filter_map(|n| {
                let mut k = HKEY::default();
                (RegOpenKeyExW(HKEY_CLASSES_ROOT, n, None, KEY_READ, &mut k) == ERROR_SUCCESS).then_some(k)
            })
            .collect();
        let mut all = pidls.0.clone();
        let dcm = DEFCONTEXTMENU {
            hwnd: HWND::default(),
            pcmcb: ManuallyDrop::new(None),
            pidlFolder: std::ptr::null_mut(),
            psf: ManuallyDrop::new(SHGetDesktopFolder().ok()),
            cidl: all.len() as u32,
            apidl: all.as_mut_ptr(),
            punkAssociationInfo: ManuallyDrop::new(None),
            cKeys: keys.len() as u32,
            aKeys: keys.as_ptr(),
        };
        let menu = SHCreateDefaultContextMenu::<IContextMenu>(&dcm);
        drop(ManuallyDrop::into_inner(dcm.psf));
        for k in keys {
            let _ = RegCloseKey(k);
        }
        menu.map_err(|e| format!("These items are in different folders, and Windows couldn't make one menu for them: {}", e.message()))
    }
}

enum Req {
    Build { paths: Vec<String>, reply: mpsc::Sender<Result<Menu, String>> },
    Invoke { session: u64, id: u32, reply: mpsc::Sender<Result<(), String>> },
    Reread { session: u64, reply: mpsc::Sender<Result<Vec<Item>, String>> },
}

/// The menu the worker built last, which is the only one it can invoke.
struct Current {
    session: u64,
    icm: IContextMenu,
    hmenu: HMENU,
}

impl Drop for Current {
    fn drop(&mut self) {
        unsafe { let _ = DestroyMenu(self.hmenu); }
    }
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
                if windows::Win32::System::Ole::OleInitialize(None).is_err() {
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                }
            }
            let mut current: Option<Current> = None;
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
                    Req::Build { mut paths, mut reply } => {
                        // Builds cost seconds (extension DLLs); when the user
                        // right-clicks again before the last one finished, only
                        // the newest request matters. Invokes pulled out while
                        // draining run afterwards, and then refer to an old menu.
                        let mut deferred = Vec::new();
                        while let Ok(next) = rx.try_recv() {
                            match next {
                                Req::Build { paths: p, reply: r } => {
                                    let _ = reply.send(Err("superseded".into()));
                                    paths = p;
                                    reply = r;
                                }
                                other => deferred.push(other),
                            }
                        }
                        current = None;
                        let session = SESSION.fetch_add(1, Ordering::SeqCst);
                        let menu = if paths.len() > menu_max() {
                            let note = format!("Shell options aren't shown for more than {} items.", fmt_count(menu_max()));
                            Menu { session, items: vec![], note: Some(note) }
                        } else {
                            match build(&paths) {
                                Ok((icm, hmenu, items)) => {
                                    current = Some(Current { session, icm, hmenu });
                                    Menu { session, items, note: None }
                                }
                                Err(e) => Menu { session, items: vec![], note: Some(e) },
                            }
                        };
                        let _ = reply.send(Ok(menu));
                        for other in deferred {
                            match other {
                                Req::Invoke { session, id, reply } => {
                                    let _ = reply.send(do_invoke(current.as_ref(), session, id));
                                }
                                Req::Reread { session, reply } => {
                                    let _ = reply.send(reread(current.as_ref(), session));
                                }
                                Req::Build { .. } => {}
                            }
                        }
                    }
                    Req::Invoke { session, id, reply } => {
                        let _ = reply.send(do_invoke(current.as_ref(), session, id));
                    }
                    Req::Reread { session, reply } => {
                        let _ = reply.send(reread(current.as_ref(), session));
                    }
                }
            }
        });
        Self { tx }
    }
}

impl ShellMenu {
    pub fn build(&self, paths: Vec<String>) -> Result<Menu, String> {
        let (rtx, rrx) = mpsc::channel();
        self.tx.send(Req::Build { paths, reply: rtx }).map_err(|e| e.to_string())?;
        rrx.recv().map_err(|e| e.to_string())?
    }
    pub fn invoke(&self, session: u64, id: u32) -> Result<(), String> {
        let (rtx, rrx) = mpsc::channel();
        self.tx.send(Req::Invoke { session, id, reply: rtx }).map_err(|e| e.to_string())?;
        rrx.recv().map_err(|e| e.to_string())?
    }
    pub fn reread(&self, session: u64) -> Result<Vec<Item>, String> {
        let (rtx, rrx) = mpsc::channel();
        self.tx.send(Req::Reread { session, reply: rtx }).map_err(|e| e.to_string())?;
        rrx.recv().map_err(|e| e.to_string())?
    }
}

fn fmt_count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The menu `session` as it is now. Some extensions add entries after the menu
/// was built, from a background thread that reports back through this
/// thread's message queue: Send to fills in its targets that way for items in
/// several folders. A real menu host shows them as they come, because its menu
/// loop pumps messages; the worker pumps while idle, so reading again sees them.
fn reread(current: Option<&Current>, session: u64) -> Result<Vec<Item>, String> {
    let c = current.filter(|c| c.session == session).ok_or("This menu is out of date. Open it again.")?;
    // The submenus were told they are opening when the menu was built.
    let mut items = read_menu(&c.icm, c.hmenu, 0, false);
    fill_verbs(&c.icm, &mut items);
    Ok(items)
}

/// Canonical verb first (survives shell-extension hooks), offset fallback.
fn do_invoke(current: Option<&Current>, session: u64, id: u32) -> Result<(), String> {
    let c = current.filter(|c| c.session == session).ok_or("This menu is out of date. Open it again.")?;
    match verb_of(&c.icm, id) {
        Some(verb) => invoke_verb(&c.icm, &verb, None).or_else(|_| invoke_offset(&c.icm, id, None)),
        None => invoke_offset(&c.icm, id, None),
    }
}

/// The selection as one shell item array (items may sit in different folders).
fn item_array(paths: &[String]) -> Result<windows::Win32::UI::Shell::IShellItemArray, String> {
    use windows::Win32::UI::Shell::SHCreateShellItemArrayFromIDLists;
    let pidls = parse(paths)?;
    let all: Vec<*const ITEMIDLIST> = pidls.0.iter().map(|p| *p as *const _).collect();
    unsafe { SHCreateShellItemArrayFromIDLists(&all) }.map_err(|e| e.message().to_string())
}

/// A shell data object for the selection (CF_HDROP and the shell's own formats).
fn data_object(paths: &[String]) -> Result<windows::Win32::System::Com::IDataObject, String> {
    use windows::Win32::UI::Shell::BHID_DataObject;
    unsafe { item_array(paths)?.BindToHandler(None, &BHID_DataObject).map_err(|e| e.message().to_string()) }
}

/// Drag items out, with the shell's drag image and drop source (which follows
/// either mouse button, so a right-drag ends in the target's menu). Must run on
/// the UI thread: Windows only gives a drag loop mouse input on the thread the
/// button went down on, and the WebView's input is attached to that one.
pub fn drag(paths: &[String], owner: isize) -> Result<(), String> {
    use windows::Win32::System::Ole::{IDropSource, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows::Win32::UI::Shell::SHDoDragDrop;
    unsafe {
        // A quick flick can let go of the button before this runs; a drag
        // started then would drop at once, wherever the pointer is.
        let down = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0;
        if !down(VK_LBUTTON) && !down(VK_RBUTTON) {
            return Ok(());
        }
        let dobj = data_object(paths)?;
        // Cancelled and completed drags are both fine outcomes.
        let _ = SHDoDragDrop(
            Some(HWND(owner as *mut _)),
            &dobj,
            None::<&IDropSource>,
            DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK,
        );
        Ok(())
    }
}

/// The shell's Properties sheet; several items get Windows' combined sheet.
/// Runs on the UI thread, which always pumps messages for the modeless sheets,
/// so a slow menu build on the worker can't hold it up.
pub fn properties(paths: &[String]) -> Result<(), String> {
    use windows::Win32::UI::Shell::{SHMultiFileProperties, ShellExecuteExW, SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW};
    let path = match paths {
        [] => return Ok(()),
        [one] => one,
        many => return unsafe { SHMultiFileProperties(&data_object(many)?, 0).map_err(|e| e.message().to_string()) },
    };
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

/// Send a menu message to the extensions behind a context menu, the way a menu
/// host must for owner-drawn items and submenus filled on demand.
fn forward(icm: &IContextMenu, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    unsafe {
        if let Ok(c3) = icm.cast::<IContextMenu3>() {
            let mut r = LRESULT(0);
            return c3.HandleMenuMsg2(msg, wp, lp, Some(&mut r)).ok().map(|_| r);
        }
        if let Ok(c2) = icm.cast::<IContextMenu2>() {
            return c2.HandleMenuMsg(msg, wp, lp).ok().map(|_| LRESULT(0));
        }
    }
    None
}

fn build(paths: &[String]) -> Result<(IContextMenu, HMENU, Vec<Item>), String> {
    let icm = context_menu(paths)?;
    unsafe {
        let hmenu = CreatePopupMenu().map_err(|e| e.to_string())?;
        if let Err(e) = icm.QueryContextMenu(hmenu, 0, ID_FIRST, ID_LAST, CMF_NORMAL).ok() {
            let _ = DestroyMenu(hmenu);
            return Err(e.message().to_string());
        }
        let mut items = read_menu(&icm, hmenu, 0, true);
        fill_verbs(&icm, &mut items);
        Ok((icm, hmenu, items))
    }
}

/// The menu's entries. `init` sends each submenu WM_INITMENUPOPUP first, as a
/// menu host does when one opens.
fn read_menu(icm: &IContextMenu, hmenu: HMENU, depth: u8, init: bool) -> Vec<Item> {
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
            let sub = if !mii.hSubMenu.is_invalid() && depth < DEPTH {
                // Send to, Open with and many extensions fill a submenu only
                // when it opens: tell them it is opening, as a menu host does.
                if init {
                    let _ = forward(icm, WM_INITMENUPOPUP, WPARAM(mii.hSubMenu.0 as usize), LPARAM(i as isize));
                }
                let s = read_menu(icm, mii.hSubMenu, depth + 1, init);
                if s.is_empty() { None } else { Some(s) }
            } else {
                None
            };
            if sep {
                out.push(Item { id: 0, text: String::new(), verb: None, sep: true, disabled: false, sub: None });
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

fn fill_verbs(icm: &IContextMenu, items: &mut [Item]) {
    for it in items {
        if !it.sep && it.id >= ID_FIRST && it.sub.is_none() {
            it.verb = verb_of(icm, it.id);
        }
        if let Some(sub) = &mut it.sub {
            fill_verbs(icm, sub);
        }
    }
}

fn verb_of(icm: &IContextMenu, id: u32) -> Option<String> {
    unsafe {
        let mut buf = [0u16; 256];
        // GCS_VERBW = 4: buffer receives wide chars despite the PSTR type.
        icm.GetCommandString((id - ID_FIRST) as usize, 4, None, windows::core::PSTR(buf.as_mut_ptr() as *mut u8), 255)
            .ok()?;
        let len = buf.iter().position(|&c| c == 0)?;
        if len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

/// Run one of the shell's verbs on items ("eject" on a drive), the way its
/// menu would. Runs on an STA thread.
pub fn run_verb(paths: &[String], verb: &str, owner: isize) -> Result<(), String> {
    let icm = context_menu(paths)?;
    unsafe {
        // Some handlers only know their verbs once the menu is built.
        let hmenu = CreatePopupMenu().map_err(|e| e.to_string())?;
        let _ = icm.QueryContextMenu(hmenu, 0, ID_FIRST, ID_LAST, CMF_NORMAL);
        let r = invoke_verb(&icm, verb, Some(HWND(owner as *mut _)));
        let _ = DestroyMenu(hmenu);
        r
    }
}

fn invoke_verb(icm: &IContextMenu, verb: &str, owner: Option<HWND>) -> Result<(), String> {
    unsafe {
        let cverb = std::ffi::CString::new(verb).map_err(|e| e.to_string())?;
        let info = CMINVOKECOMMANDINFO {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
            hwnd: owner.unwrap_or_default(),
            lpVerb: PCSTR(cverb.as_ptr() as *const u8),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        icm.InvokeCommand(&info).map_err(|e| e.message().to_string())
    }
}

fn invoke_offset(icm: &IContextMenu, id: u32, owner: Option<HWND>) -> Result<(), String> {
    unsafe {
        let info = CMINVOKECOMMANDINFO {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
            hwnd: owner.unwrap_or_default(),
            lpVerb: PCSTR((id - ID_FIRST) as usize as *const u8),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        icm.InvokeCommand(&info).map_err(|e| e.message().to_string())
    }
}

/// While Windows' own menu is open: hand the menu messages to the extensions.
unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, data: usize) -> LRESULT {
    use windows::Win32::UI::Shell::DefSubclassProc;
    use windows::Win32::UI::WindowsAndMessaging::{WM_DRAWITEM, WM_MEASUREITEM, WM_MENUCHAR};
    let menu_msg = matches!(msg, WM_INITMENUPOPUP | WM_MENUCHAR) || (matches!(msg, WM_DRAWITEM | WM_MEASUREITEM) && wp.0 == 0);
    if menu_msg {
        let icm = unsafe { &*(data as *const IContextMenu) };
        if let Some(r) = forward(icm, msg, wp, lp) {
            return r;
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
}

/// Show a popup menu at a screen point and return the picked id (0 for none).
/// The window comes to the front, so a click elsewhere closes the menu. The
/// focus passes through it to WebView2's window (wry forwards WM_SETFOCUS),
/// whose input queue is joined to this thread's, so the menu loop gets the
/// keys. Keeping the focus on this window instead loses them. Runs on the UI
/// thread.
pub fn track(top: HWND, menu: HMENU, x: i32, y: i32) -> u32 {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
    use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, TrackPopupMenuEx, TPM_RETURNCMD, TPM_RIGHTBUTTON};
    unsafe {
        let focus = GetFocus();
        let _ = SetForegroundWindow(top);
        let _ = SetFocus(Some(top));
        let picked = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, x, y, top, None).0 as u32;
        if !focus.is_invalid() {
            let _ = SetFocus(Some(focus));
        }
        picked
    }
}

/// Windows' own context menu for the items, at a point in the window's client
/// area (physical pixels), with everything extensions add, owner-drawn items
/// and submenus included. A picked verb in `handled` (lowercase) comes back for
/// the page to run its own way (the shell's version needs Explorer's view, or
/// would bypass Filezy's undo); anything else is invoked here. Runs on the UI thread.
pub fn native(paths: &[String], owner: isize, x: i32, y: i32, extended: bool, handled: &[String]) -> Result<Option<String>, String> {
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::Shell::{RemoveWindowSubclass, SetWindowSubclass};
    const HOST: usize = 0x46_5a_4d_4e; // subclass id
    let top = HWND(owner as *mut _);
    let icm = context_menu(paths)?;
    unsafe {
        let hmenu = CreatePopupMenu().map_err(|e| e.to_string())?;
        // CMF_CANRENAME: Filezy renames in place, so the menu offers Rename as in Explorer.
        let flags = CMF_NORMAL | CMF_CANRENAME | if extended { CMF_EXTENDEDVERBS } else { 0 };
        if let Err(e) = icm.QueryContextMenu(hmenu, 0, ID_FIRST, ID_LAST, flags).ok() {
            let _ = DestroyMenu(hmenu);
            return Err(e.message().to_string());
        }
        let mut pt = POINT { x, y };
        let _ = ClientToScreen(top, &mut pt);
        let _ = SetWindowSubclass(top, Some(host_proc), HOST, &icm as *const IContextMenu as usize);
        let picked = track(top, hmenu, pt.x, pt.y);
        let _ = RemoveWindowSubclass(top, Some(host_proc), HOST);
        let res = if picked < ID_FIRST {
            Ok(None)
        } else {
            match verb_of(&icm, picked).map(|v| v.to_lowercase()) {
                Some(v) if handled.contains(&v) => Ok(Some(v)),
                _ => invoke_offset(&icm, picked, Some(top)).map(|_| None),
            }
        };
        let _ = DestroyMenu(hmenu);
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_read_like_explorer() {
        assert_eq!(fmt_count(10_000), "10,000");
        assert_eq!(fmt_count(999), "999");
        assert_eq!(fmt_count(1_234_567), "1,234,567");
    }
}
