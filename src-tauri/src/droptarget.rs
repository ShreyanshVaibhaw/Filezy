//! The window's OLE drop target.
//!
//! Tauri's own target tells Windows that every drop is a copy and leaves the
//! rest to the page. This one replaces it, so a drag over Filezy behaves like a
//! drag over Explorer: the cursor and the caption under the drag image say what
//! a drop will do ("Move to Documents"), Ctrl, Shift and Alt change that while
//! dragging, a right-drag ends in Explorer's Copy here / Move here menu, and
//! moves are carried out here (an "optimized move"), so the source never deletes
//! anything itself.
//!
//! The page knows what is under the pointer. The target sends it every pointer
//! position as a "drag" event; the page answers with `drop_dest` whenever the
//! destination under the pointer changes. The target decides the effect.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use windows::core::{implement, w, Ref, BOOL, PCWSTR};
use windows::Win32::Foundation::{HGLOBAL, HWND, LPARAM, POINT, POINTL};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{
    CoCreateInstance, IDataObject, CLSCTX_INPROC_SERVER, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0,
    TYMED_HGLOBAL,
};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop, CF_HDROP, DROPEFFECT,
    DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE,
};
use windows::Win32::System::SystemServices::{MODIFIERKEYS_FLAGS, MK_CONTROL, MK_RBUTTON, MK_SHIFT};
use windows::Win32::UI::Shell::{
    IDropTargetHelper, CFSTR_DROPDESCRIPTION, CFSTR_LOGICALPERFORMEDDROPEFFECT,
    CFSTR_PERFORMEDDROPEFFECT, CLSID_DragDropHelper, DROPDESCRIPTION, DROPIMAGETYPE, DROPIMAGE_COPY,
    DROPIMAGE_INVALID, DROPIMAGE_LINK, DROPIMAGE_MOVE, HDROP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, EnumChildWindows, SetMenuDefaultItem, MF_SEPARATOR, MF_STRING,
};

/// Alt in an OLE key state; Windows' headers define it, windows-rs doesn't.
const MK_ALT: u32 = 0x20;
/// The destination the page uses for the Recycle Bin.
pub const RECYCLE: &str = "::RecycleBin";

/// Where a drop at the pointer would land, as the page last said.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Dest {
    /// A folder, or RECYCLE.
    pub path: String,
    /// The name in the caption: "Move to <label>".
    pub label: String,
}

static DEST: Mutex<Option<Dest>> = Mutex::new(None);

pub fn set_dest(dest: Option<Dest>) {
    *DEST.lock().unwrap() = dest;
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Event {
    /// A drag came in; nothing about the last one applies any more.
    Enter,
    /// Pointer position in physical pixels from the window's client origin.
    Over { x: i32, y: i32 },
    Leave,
    Drop { paths: Vec<String>, dest: String, op: &'static str },
}

/// What one drag over the window carries, from DragEnter to Drop or DragLeave.
#[derive(Default)]
struct Drag {
    data: Option<IDataObject>,
    paths: Vec<String>,
    /// Volume of each distinct source folder.
    volumes: Vec<Option<String>>,
    /// Effects the source permits.
    allowed: DROPEFFECT,
    /// Started with the right button: the drop asks what to do.
    right: bool,
    /// The caption last put on the drag image, to skip setting it again.
    caption: Option<(i32, String)>,
    /// Volume of each destination seen during this drag.
    dest_volumes: HashMap<String, Option<String>>,
}

#[implement(IDropTarget)]
struct Target {
    /// The main window: client coordinates, the drag image and the menu.
    top: HWND,
    app: AppHandle,
    helper: Option<IDropTargetHelper>,
    drag: RefCell<Drag>,
}

/// The right-drag menu: Copy here, Move here, Create shortcuts here and Cancel,
/// the drop's own effect in bold, at a screen point. Runs on the UI thread.
fn ask(top: HWND, pt: POINTL, default: DROPEFFECT, allowed: DROPEFFECT) -> DROPEFFECT {
    let items = [
        (DROPEFFECT_COPY, w!("&Copy here")),
        (DROPEFFECT_MOVE, w!("&Move here")),
        (DROPEFFECT_LINK, w!("Create &shortcuts here")),
    ];
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return DROPEFFECT_NONE };
        for (e, text) in items {
            if has(allowed, e) {
                let _ = AppendMenuW(menu, MF_STRING, e.0 as usize, text);
            }
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, 1000, w!("Cancel"));
        if default != DROPEFFECT_NONE {
            let _ = SetMenuDefaultItem(menu, default.0, 0);
        }
        let picked = crate::shellmenu::track(top, menu, pt.x, pt.y);
        let _ = DestroyMenu(menu);
        match picked {
            1 => DROPEFFECT_COPY,
            2 => DROPEFFECT_MOVE,
            4 => DROPEFFECT_LINK,
            _ => DROPEFFECT_NONE,
        }
    }
}

/// What a drop does at a destination: the page's operation name, if any.
fn op_of(e: DROPEFFECT, dest: &Dest) -> Option<&'static str> {
    match e {
        DROPEFFECT_MOVE if dest.path == RECYCLE => Some("recycle"),
        DROPEFFECT_MOVE => Some("move"),
        DROPEFFECT_COPY => Some("copy"),
        DROPEFFECT_LINK => Some("link"),
        _ => None,
    }
}

/// Put our drop target on the main window and every window inside it (the
/// WebView's), in place of the ones WebView2 and Tauri registered. Must run on
/// the UI thread, which is where OLE then calls the target.
pub fn install(app: &AppHandle, top: isize) {
    let top = HWND(top as *mut _);
    let helper = unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER) }.ok();
    let target: IDropTarget = Target { top, app: app.clone(), helper, drag: RefCell::default() }.into();
    if crate::e2e() {
        crate::e2edrag::set_target(&target);
    }
    let mut windows = vec![top];
    unsafe extern "system" fn collect(hwnd: HWND, lp: LPARAM) -> BOOL {
        unsafe { (*(lp.0 as *mut Vec<HWND>)).push(hwnd) };
        true.into()
    }
    unsafe {
        let _ = EnumChildWindows(Some(top), Some(collect), LPARAM(&mut windows as *mut _ as isize));
        for hwnd in windows {
            // Windows owned by another process (WebView2's browser) refuse; skip them.
            let _ = RevokeDragDrop(hwnd);
            let _ = RegisterDragDrop(hwnd, &target);
        }
    }
}

/// The file paths in a data object, if it carries files.
fn paths_of(data: &IDataObject) -> Vec<String> {
    let fmt = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    unsafe {
        let Ok(mut medium) = data.GetData(&fmt) else { return vec![] };
        let paths = crate::clip::hdrop_paths(HDROP(medium.u.hGlobal.0));
        ReleaseStgMedium(&mut medium);
        paths
    }
}

/// Hand an HGLOBAL holding `bytes` to the data object under a registered format.
fn set_data(data: &IDataObject, format: PCWSTR, bytes: &[u8]) {
    unsafe {
        let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else { return };
        let p = GlobalLock(h) as *mut u8;
        if p.is_null() {
            return;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        let _ = GlobalUnlock(h);
        let fmt = FORMATETC {
            cfFormat: RegisterClipboardFormatW(format) as u16,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let mut medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: HGLOBAL(h.0) },
            pUnkForRelease: std::mem::ManuallyDrop::new(None),
        };
        // On success the data object owns the memory; otherwise free it here.
        if data.SetData(&fmt, &medium, true).is_err() {
            ReleaseStgMedium(&mut medium);
        }
    }
}

fn effect_bytes(e: DROPEFFECT) -> [u8; 4] {
    e.0.to_le_bytes()
}

fn has(set: DROPEFFECT, e: DROPEFFECT) -> bool {
    set.0 & e.0 != 0
}

impl Target {
    fn client(&self, pt: &POINTL) -> (i32, i32) {
        let mut p = POINT { x: pt.x, y: pt.y };
        unsafe { let _ = ScreenToClient(self.top, &mut p); }
        (p.x, p.y)
    }

    fn emit(&self, ev: Event) {
        let _ = self.app.emit("drag", ev);
    }

    /// What a drop with these keys would do at the page's current destination,
    /// by Explorer's rules: the same volume moves and another copies, Ctrl
    /// copies, Shift moves, Alt or Ctrl+Shift links, and nothing goes into
    /// itself or moves to where it already is.
    fn decide(&self, keys: MODIFIERKEYS_FLAGS) -> (DROPEFFECT, Option<Dest>) {
        let dest = DEST.lock().unwrap().clone();
        let mut d = self.drag.borrow_mut();
        let Some(dest) = dest else { return (DROPEFFECT_NONE, None) };
        if d.paths.is_empty() {
            return (DROPEFFECT_NONE, None);
        }
        if dest.path == RECYCLE {
            let e = if has(d.allowed, DROPEFFECT_MOVE) { DROPEFFECT_MOVE } else { DROPEFFECT_NONE };
            return (e, Some(dest));
        }
        let to = dest.path.to_lowercase();
        let into_itself = d.paths.iter().any(|p| {
            let p = p.to_lowercase();
            to == p || to.starts_with(&format!("{}\\", p.trim_end_matches('\\')))
        });
        if into_itself {
            return (DROPEFFECT_NONE, Some(dest));
        }
        let (ctrl, shift, alt) = (keys.0 & MK_CONTROL.0 != 0, keys.0 & MK_SHIFT.0 != 0, keys.0 & MK_ALT != 0);
        let wanted = if alt || (ctrl && shift) {
            DROPEFFECT_LINK
        } else if ctrl {
            DROPEFFECT_COPY
        } else if shift {
            DROPEFFECT_MOVE
        } else {
            let vol = d.dest_volumes.entry(to.clone()).or_insert_with(|| crate::paths::volume_id(&dest.path)).clone();
            if vol.is_some() && d.volumes.iter().all(|v| *v == vol) { DROPEFFECT_MOVE } else { DROPEFFECT_COPY }
        };
        let already_there =
            d.paths.iter().all(|p| crate::paths::parent(p).is_some_and(|parent| parent.to_lowercase() == to));
        if wanted == DROPEFFECT_MOVE && already_there {
            return (DROPEFFECT_NONE, Some(dest));
        }
        // The source decides what it permits; fall back the way Explorer does.
        let e = [wanted, DROPEFFECT_COPY, DROPEFFECT_MOVE, DROPEFFECT_LINK]
            .into_iter()
            .find(|e| has(d.allowed, *e))
            .unwrap_or(DROPEFFECT_NONE);
        (e, Some(dest))
    }

    /// The caption under the drag image: "Move to Documents".
    fn caption(&self, effect: DROPEFFECT, dest: Option<&Dest>) {
        let (ty, message): (DROPIMAGETYPE, &str) = match (effect, dest) {
            (DROPEFFECT_MOVE, Some(_)) => (DROPIMAGE_MOVE, "Move to %1"),
            (DROPEFFECT_COPY, Some(_)) => (DROPIMAGE_COPY, "Copy to %1"),
            (DROPEFFECT_LINK, Some(_)) => (DROPIMAGE_LINK, "Create link in %1"),
            _ => (DROPIMAGE_INVALID, ""),
        };
        let insert = if ty == DROPIMAGE_INVALID { String::new() } else { dest.map(|d| d.label.clone()).unwrap_or_default() };
        let mut d = self.drag.borrow_mut();
        let key = (ty.0, insert.clone());
        if d.caption.as_ref() == Some(&key) {
            return;
        }
        d.caption = Some(key);
        let Some(data) = d.data.clone() else { return };
        drop(d);
        // DROPDESCRIPTION is packed, so its arrays are filled before it is built.
        let text = |s: &str| {
            let mut buf = [0u16; 260];
            for (i, c) in s.encode_utf16().take(259).enumerate() {
                buf[i] = c;
            }
            buf
        };
        let desc = DROPDESCRIPTION { r#type: ty, szMessage: text(message), szInsert: text(&insert) };
        let bytes = unsafe {
            std::slice::from_raw_parts(&desc as *const _ as *const u8, std::mem::size_of::<DROPDESCRIPTION>())
        };
        set_data(&data, CFSTR_DROPDESCRIPTION, bytes);
    }

    /// Explorer's right-drag menu; returns the chosen effect, or none.
    fn over(&self, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) {
        let (x, y) = self.client(pt);
        self.emit(Event::Over { x, y });
        let (e, dest) = self.decide(keys);
        self.caption(e, dest.as_ref());
        unsafe { *effect = e };
        if let Some(h) = &self.helper {
            unsafe { let _ = h.DragOver(&POINT { x: pt.x, y: pt.y }, e); }
        }
    }

    fn end(&self) {
        // Leave no caption of ours on the data object for the next target.
        self.caption(DROPEFFECT_NONE, None);
        *self.drag.borrow_mut() = Drag::default();
        set_dest(None);
    }
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let data = data.ok()?.clone();
        let paths = paths_of(&data);
        let mut dirs: Vec<String> = paths.iter().filter_map(|p| crate::paths::parent(p)).collect();
        dirs.sort_by_key(|d| d.to_lowercase());
        dirs.dedup_by_key(|d| d.to_lowercase());
        *self.drag.borrow_mut() = Drag {
            data: Some(data.clone()),
            volumes: dirs.iter().map(|d| crate::paths::volume_id(d)).collect(),
            paths,
            allowed: unsafe { *effect },
            right: keys.0 & MK_RBUTTON.0 != 0,
            ..Default::default()
        };
        set_dest(None);
        self.emit(Event::Enter);
        if let Some(h) = &self.helper {
            unsafe { let _ = h.DragEnter(self.top, &data, &POINT { x: pt.x, y: pt.y }, DROPEFFECT_NONE); }
        }
        self.over(keys, pt, effect);
        Ok(())
    }

    fn DragOver(&self, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        self.over(keys, pt, effect);
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        if let Some(h) = &self.helper {
            unsafe { let _ = h.DragLeave(); }
        }
        self.emit(Event::Leave);
        self.end();
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let (mut e, dest) = self.decide(keys);
        if let (Some(h), Ok(data)) = (&self.helper, data.ok()) {
            unsafe { let _ = h.Drop(data, &POINT { x: pt.x, y: pt.y }, e); }
        }
        let (right, allowed, paths, data) = {
            let d = self.drag.borrow();
            (d.right, d.allowed, d.paths.clone(), d.data.clone())
        };
        if let (true, Some(d), false) = (right, &dest, paths.is_empty()) {
            e = ask(self.top, *pt, e, if d.path == RECYCLE { DROPEFFECT_MOVE } else { allowed });
        }
        let op = dest.as_ref().and_then(|d| op_of(e, d));
        // Moves happen here, so tell the source not to delete anything: an
        // optimized move reports something other than MOVE back.
        let reported = if e == DROPEFFECT_MOVE { DROPEFFECT_NONE } else if op.is_some() { e } else { DROPEFFECT_NONE };
        if let (Some(data), Some(_)) = (&data, op) {
            set_data(data, CFSTR_PERFORMEDDROPEFFECT, &effect_bytes(reported));
            set_data(data, CFSTR_LOGICALPERFORMEDDROPEFFECT, &effect_bytes(e));
        }
        unsafe { *effect = reported };
        if let (Some(op), Some(dest)) = (op, dest) {
            self.emit(Event::Drop { paths, dest: dest.path, op });
        } else {
            self.emit(Event::Leave);
        }
        self.end();
        Ok(())
    }
}
