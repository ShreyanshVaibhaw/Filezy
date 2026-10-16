//! File operations through Explorer's own engine (IFileOperation): native
//! progress, conflict and confirmation dialogs, Recycle Bin, long paths.
//!
//! Each operation runs on its own STA thread, so a long copy never holds up a
//! context menu, and reports exactly what happened per item: done, skipped,
//! failed or never attempted, whether it replaced an existing item, and where
//! a recycled item now sits in the Recycle Bin.
use crate::fs_list::wide;
use crate::paths;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use windows::core::{Ref, HRESULT, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{
    FileOperation, IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem,
    SHCreateItemFromParsingName, ShellExecuteW, FILEOPERATION_FLAGS, SIGDN_DESKTOPABSOLUTEPARSING,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const FOF_RENAMEONCOLLISION: u32 = 0x8;
const FOF_NOCONFIRMATION: u32 = 0x10;
const FOF_ALLOWUNDO: u32 = 0x40;
const FOFX_RECYCLEONDELETE: u32 = 0x0008_0000;
const FOFX_ADDUNDORECORD: u32 = 0x2000_0000;

/// The copy engine's "the user cancelled" codes.
const COPYENGINE_E_USER_CANCELLED: i32 = 0x8027_0000u32 as i32;
const E_ABORT: i32 = 0x8000_4004u32 as i32;
/// The success code for an item the user chose not to process ("Skip").
/// Other COPYENGINE_S_* codes (merged, moved as a whole, kept both) are done.
const COPYENGINE_S_USER_IGNORED: i32 = 0x0027_0005;
/// A conflict was queued for the user; the item's real outcome is reported again later.
const COPYENGINE_S_PENDING: i32 = 0x0027_000B;

pub enum Op {
    Copy { srcs: Vec<String>, dest: String },
    Move { srcs: Vec<String>, dest: String },
    /// To the Recycle Bin (where the volume has one).
    Recycle { paths: Vec<String> },
    /// Permanently, after Windows' own confirmation.
    Delete { paths: Vec<String> },
    /// Put items back at exact paths (undo of a move or rename), across volumes too.
    MoveTo { items: Vec<(String, String)> },
    /// Move freshly extracted items from their staging folder into place, with
    /// Windows' conflict questions but no Windows undo record: undoing the
    /// move would put them back into a folder that is about to be deleted.
    Place { srcs: Vec<String>, dest: String },
}

impl Op {
    fn kind(&self) -> &'static str {
        match self {
            Op::Copy { .. } => "copy",
            Op::Move { .. } => "move",
            Op::Recycle { .. } => "recycle",
            Op::Delete { .. } => "delete",
            Op::MoveTo { .. } => "move",
            Op::Place { .. } => "move",
        }
    }
    fn sources(&self) -> Vec<String> {
        match self {
            Op::Copy { srcs, .. } | Op::Move { srcs, .. } | Op::Place { srcs, .. } => srcs.clone(),
            Op::Recycle { paths } | Op::Delete { paths } => paths.clone(),
            Op::MoveTo { items } => items.iter().map(|(from, _)| from.clone()).collect(),
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Done,
    Skipped,
    Failed,
    /// The engine never reached it (cancelled or stopped earlier).
    Unattempted,
}

#[derive(Serialize, Clone, Debug)]
pub struct Item {
    pub src: String,
    pub dst: String,
    pub state: State,
    pub error: Option<String>,
    /// The destination already existed and was overwritten or merged into, so
    /// removing the destination would not bring the old contents back.
    pub replaced: bool,
    /// Where a recycled item now sits in the Recycle Bin (its parsing name).
    pub bin_id: Option<String>,
    /// Destination size and modified time right after the operation, so undo
    /// can tell when the item was changed since.
    pub size: u64,
    pub modified: u64,
    /// The engine's HRESULT for this item, for diagnostics and bug reports.
    pub hr: i32,
}

#[derive(Serialize, Clone, Debug)]
pub struct OpResult {
    pub kind: &'static str,
    pub items: Vec<Item>,
    /// Some work was stopped before completion (cancel, skip, or failure).
    pub aborted: bool,
    /// The user cancelled.
    pub cancelled: bool,
    /// Why the operation as a whole failed to run.
    pub error: Option<String>,
    /// Sub-items processed inside folders.
    pub children: usize,
    /// For a cut-paste, what happened to the clipboard afterwards: "cleared",
    /// "rewritten" (it keeps the items that didn't move), or "changed elsewhere"
    /// (another program replaced it meanwhile, so it was left alone).
    pub clipboard: Option<&'static str>,
}

fn classify(hr: HRESULT) -> (State, Option<String>) {
    match hr.0 {
        COPYENGINE_S_USER_IGNORED => (State::Skipped, None),
        COPYENGINE_E_USER_CANCELLED | E_ABORT => (State::Unattempted, None),
        c if c >= 0 => (State::Done, None),
        _ => (State::Failed, Some(windows::core::Error::from(hr).message().to_string())),
    }
}

pub fn item_path(it: &IShellItem, sigdn: windows::Win32::UI::Shell::SIGDN) -> String {
    unsafe {
        it.GetDisplayName(sigdn)
            .ok()
            .map(|p| {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                s
            })
            .unwrap_or_default()
    }
}

fn ref_path(it: Ref<IShellItem>) -> String {
    it.as_ref().map(|i| paths::to_display(&item_path(i, SIGDN_FILESYSPATH))).unwrap_or_default()
}

#[derive(Default)]
struct Log {
    /// Requested sources, lower-cased, to tell top-level items from their children.
    top: HashSet<String>,
    items: Vec<Item>,
    /// Destinations that already existed when the engine reached them.
    existed: HashSet<String>,
    children: usize,
}

impl Log {
    fn is_top(&self, src: &str) -> bool {
        self.top.contains(&src.to_lowercase())
    }

    fn pre(&mut self, src: &str, dest: Ref<IShellItem>, name: &PCWSTR) {
        if !self.is_top(src) {
            return;
        }
        let dst = target(dest, name, src);
        if !dst.is_empty() && paths::attributes(&dst).is_ok() {
            self.existed.insert(dst.to_lowercase());
        }
    }

    fn post(&mut self, src: String, dst: String, hr: HRESULT, bin_id: Option<String>) {
        if hr.0 == COPYENGINE_S_PENDING {
            return; // not an outcome yet: the conflict dialog decides it
        }
        if !self.is_top(&src) {
            if hr.is_ok() {
                self.children += 1;
            }
            return;
        }
        let (state, error) = classify(hr);
        let replaced = state == State::Done && !dst.is_empty() && self.existed.contains(&dst.to_lowercase());
        let item = Item { src, dst, state, error, replaced, bin_id, size: 0, modified: 0, hr: hr.0 };
        // The engine can report an item more than once; its last word counts.
        match self.items.iter_mut().find(|i| i.src.eq_ignore_ascii_case(&item.src)) {
            Some(prev) => *prev = item,
            None => self.items.push(item),
        }
    }
}

/// dest folder + explicit name (or the source's own name).
fn target(dest: Ref<IShellItem>, name: &PCWSTR, src: &str) -> String {
    let folder = ref_path(dest);
    if folder.is_empty() {
        return String::new();
    }
    let leaf = unsafe {
        if !name.is_null() && !name.as_wide().is_empty() {
            name.to_string().unwrap_or_default()
        } else {
            paths::file_name(src).to_string()
        }
    };
    paths::join(&folder, &leaf)
}

#[windows::core::implement(IFileOperationProgressSink)]
struct Sink(Arc<Mutex<Log>>);

#[allow(non_snake_case)]
impl IFileOperationProgressSink_Impl for Sink_Impl {
    fn StartOperations(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, _hr: HRESULT) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreRenameItem(&self, _f: u32, _i: Ref<IShellItem>, _n: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostRenameItem(
        &self,
        _f: u32,
        item: Ref<IShellItem>,
        _n: &PCWSTR,
        hr: HRESULT,
        new_item: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        self.0.lock().unwrap().post(ref_path(item), ref_path(new_item), hr, None);
        Ok(())
    }
    fn PreMoveItem(&self, _f: u32, item: Ref<IShellItem>, dest: Ref<IShellItem>, name: &PCWSTR) -> windows::core::Result<()> {
        self.0.lock().unwrap().pre(&ref_path(item), dest, name);
        Ok(())
    }
    fn PostMoveItem(
        &self,
        _f: u32,
        item: Ref<IShellItem>,
        dest: Ref<IShellItem>,
        name: &PCWSTR,
        hr: HRESULT,
        new_item: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        let src = ref_path(item);
        let dst = match ref_path(new_item) {
            s if !s.is_empty() => s,
            _ if hr.0 == 0 => target(dest, name, &src),
            _ => String::new(),
        };
        self.0.lock().unwrap().post(src, dst, hr, None);
        Ok(())
    }
    fn PreCopyItem(&self, _f: u32, item: Ref<IShellItem>, dest: Ref<IShellItem>, name: &PCWSTR) -> windows::core::Result<()> {
        self.0.lock().unwrap().pre(&ref_path(item), dest, name);
        Ok(())
    }
    fn PostCopyItem(
        &self,
        _f: u32,
        item: Ref<IShellItem>,
        dest: Ref<IShellItem>,
        name: &PCWSTR,
        hr: HRESULT,
        new_item: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        // Prefer the shell's new item: with "Keep both" its name differs from
        // the source's. Only a plain success may fall back to folder + name.
        let src = ref_path(item);
        let dst = match ref_path(new_item) {
            s if !s.is_empty() => s,
            _ if hr.0 == 0 => target(dest, name, &src),
            _ => String::new(),
        };
        self.0.lock().unwrap().post(src, dst, hr, None);
        Ok(())
    }
    fn PreDeleteItem(&self, _f: u32, _i: Ref<IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostDeleteItem(&self, _f: u32, item: Ref<IShellItem>, hr: HRESULT, new_item: Ref<IShellItem>) -> windows::core::Result<()> {
        // For a recycle, the new item is the entry in the Recycle Bin.
        let bin_id = new_item.as_ref().map(|i| item_path(i, SIGDN_DESKTOPABSOLUTEPARSING)).filter(|s| !s.is_empty());
        self.0.lock().unwrap().post(ref_path(item), String::new(), hr, bin_id);
        Ok(())
    }
    fn PreNewItem(&self, _f: u32, _d: Ref<IShellItem>, _n: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostNewItem(
        &self,
        _f: u32,
        _d: Ref<IShellItem>,
        _n: &PCWSTR,
        _t: &PCWSTR,
        _attrs: u32,
        _hr: HRESULT,
        _new_item: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, _total: u32, _sofar: u32) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

fn shell_item(path: &str) -> windows::core::Result<IShellItem> {
    let w = wide(path);
    unsafe { SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None) }
}

/// Size and modified time (ms) of a path, for undo's "changed since" check.
pub fn stamp(path: &str) -> (u64, u64) {
    std::fs::symlink_metadata(paths::to_native(path))
        .map(|m| {
            let ms = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            (if m.is_dir() { 0 } else { m.len() }, ms)
        })
        .unwrap_or((0, 0))
}

/// Queue the work on `fo`. Items the shell can't even parse fail on their own
/// instead of sinking the whole batch.
fn queue(fo: &IFileOperation, op: &Op, log: &mut Log) -> Result<(), String> {
    let err = |e: windows::core::Error| e.message().to_string();
    let mut add = |src: &str, r: windows::core::Result<()>| {
        if let Err(e) = r {
            log.items.push(Item {
                src: src.to_string(),
                dst: String::new(),
                state: State::Failed,
                error: Some(e.message().to_string()),
                replaced: false,
                bin_id: None,
                size: 0,
                modified: 0,
                hr: 0,
            });
        }
    };
    unsafe {
        match op {
            Op::Copy { srcs, dest } | Op::Move { srcs, dest } | Op::Place { srcs, dest } => {
                let dest_item = shell_item(dest).map_err(err)?;
                let copy = matches!(op, Op::Copy { .. });
                for s in srcs {
                    let r = shell_item(s).and_then(|it| {
                        if copy {
                            fo.CopyItem(&it, &dest_item, PCWSTR::null(), None)
                        } else {
                            fo.MoveItem(&it, &dest_item, PCWSTR::null(), None)
                        }
                    });
                    add(s, r);
                }
            }
            Op::Recycle { paths } | Op::Delete { paths } => {
                for p in paths {
                    add(p, shell_item(p).and_then(|it| fo.DeleteItem(&it, None)));
                }
            }
            Op::MoveTo { items } => {
                for (from, to) in items {
                    let parent = paths::parent(to).unwrap_or_default();
                    let name = wide(paths::file_name(to));
                    let r = shell_item(from).and_then(|it| {
                        let folder = shell_item(&parent)?;
                        fo.MoveItem(&it, &folder, PCWSTR(name.as_ptr()), None)
                    });
                    add(from, r);
                }
            }
        }
    }
    Ok(())
}

fn perform(op: &Op, owner: isize) -> OpResult {
    let mut result =
        OpResult { kind: op.kind(), items: vec![], aborted: false, cancelled: false, error: None, children: 0, clipboard: None };
    let log = Arc::new(Mutex::new(Log {
        top: op.sources().iter().map(|s| s.to_lowercase()).collect(),
        ..Default::default()
    }));
    let mut run = || -> Result<bool, String> {
        unsafe {
            let fo: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER).map_err(|e| e.message().to_string())?;
            if owner != 0 {
                let _ = fo.SetOwnerWindow(HWND(owner as *mut _));
            }
            let flags = match op {
                // Pasting into the items' own folder copies with " - Copy" names, like Explorer.
                Op::Copy { srcs, dest } if srcs.iter().all(|s| paths::parent(s).is_some_and(|p| p.eq_ignore_ascii_case(dest))) => {
                    FOF_ALLOWUNDO | FOFX_ADDUNDORECORD | FOF_RENAMEONCOLLISION
                }
                // Explorer doesn't ask before recycling by default; recycle even big files.
                Op::Recycle { .. } => FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOFX_RECYCLEONDELETE,
                // Permanent delete: no undo flag, so Windows asks "permanently delete?".
                Op::Delete { .. } => 0,
                Op::Place { .. } => 0,
                _ => FOF_ALLOWUNDO | FOFX_ADDUNDORECORD,
            };
            fo.SetOperationFlags(FILEOPERATION_FLAGS(flags)).map_err(|e| e.message().to_string())?;
            let sink: IFileOperationProgressSink = Sink(log.clone()).into();
            let cookie = fo.Advise(&sink).unwrap_or(0);
            queue(&fo, op, &mut log.lock().unwrap())?;
            let performed = fo.PerformOperations();
            let aborted = fo.GetAnyOperationsAborted().map(|b| b.as_bool()).unwrap_or(false);
            if cookie != 0 {
                let _ = fo.Unadvise(cookie);
            }
            match performed {
                Ok(()) => Ok(aborted),
                Err(e) if e.code().0 == COPYENGINE_E_USER_CANCELLED || e.code().0 == E_ABORT => {
                    result.cancelled = true;
                    Ok(true)
                }
                Err(e) => Err(format!("{} ({:#x})", e.message(), e.code().0)),
            }
        }
    };
    match run() {
        Ok(aborted) => result.aborted = aborted,
        Err(e) => {
            result.aborted = true;
            result.error = Some(e);
        }
    }
    let mut log = std::mem::take(&mut *log.lock().unwrap());
    // Requested items the engine never reported on were not attempted.
    let seen: HashSet<String> = log.items.iter().map(|i| i.src.to_lowercase()).collect();
    for s in op.sources() {
        if !seen.contains(&s.to_lowercase()) {
            log.items.push(Item {
                src: s,
                dst: String::new(),
                state: State::Unattempted,
                error: None,
                replaced: false,
                bin_id: None,
                size: 0,
                modified: 0,
                hr: 0,
            });
        }
    }
    for it in &mut log.items {
        if it.state == State::Done && !it.dst.is_empty() {
            (it.size, it.modified) = stamp(&it.dst);
        }
    }
    result.items = log.items;
    result.children = log.children;
    result
}

/// Run an operation on a fresh STA thread and wait for it. `owner` is the
/// window the native dialogs belong to.
pub fn run(op: Op, owner: isize) -> OpResult {
    std::thread::spawn(move || {
        unsafe {
            if windows::Win32::System::Ole::OleInitialize(None).is_err() {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            }
        }
        let r = perform(&op, owner);
        unsafe { windows::Win32::System::Ole::OleUninitialize() };
        r
    })
    .join()
    .unwrap_or_else(|_| OpResult {
        kind: "copy",
        items: vec![],
        aborted: true,
        cancelled: false,
        error: Some("The file operation stopped unexpectedly.".into()),
        children: 0,
        clipboard: None,
    })
}

pub fn shell_open(path: &str) -> Result<(), String> {
    shell_open_verb(path, "open")
}

pub fn shell_open_verb(path: &str, verb: &str) -> Result<(), String> {
    let wpath = wide(path);
    let verb = wide(verb);
    let h = unsafe {
        ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(wpath.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL)
    };
    // Per docs, values > 32 indicate success.
    if h.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!("Could not open {path}"))
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Renamed {
    pub from: String,
    pub to: String,
    pub error: Option<String>,
}

/// Rename several items at once. Each goes through a temporary name first, so
/// a target that is another batch item's current name (a swap, or "x (2)"
/// already selected) can't collide halfway. A target that exists outside the
/// batch is refused for that item alone.
pub fn rename_many(items: &[(String, String)]) -> Vec<Renamed> {
    let sources: HashSet<String> = items.iter().map(|(f, _)| f.to_lowercase()).collect();
    let mut taken: HashSet<String> = HashSet::new();
    let mut out: Vec<Renamed> = items
        .iter()
        .map(|(from, name)| {
            let to = (|| -> Result<String, String> {
                let name = paths::check_name(name)?;
                let parent = paths::parent(from).ok_or("This item can't be renamed.")?;
                let to = paths::join(&parent, &name);
                if !taken.insert(to.to_lowercase()) {
                    return Err(format!("Two items would both be named '{name}'."));
                }
                if !sources.contains(&to.to_lowercase()) && paths::attributes(&to).is_ok() {
                    return Err(format!("There is already a file with the name '{name}' in this location."));
                }
                Ok(to)
            })();
            match to {
                Ok(to) => Renamed { from: from.clone(), to, error: None },
                Err(e) => Renamed { from: from.clone(), to: String::new(), error: Some(e) },
            }
        })
        .collect();
    let native = |p: &str| paths::to_native(p);
    let mut temps: Vec<Option<String>> = vec![None; out.len()];
    for (i, r) in out.iter_mut().enumerate() {
        if r.error.is_some() || r.to == r.from {
            continue;
        }
        let parent = paths::parent(&r.from).unwrap_or_default();
        let tmp = paths::join(&parent, &format!(".filezy-rename-{}-{i}.tmp", std::process::id()));
        match std::fs::rename(native(&r.from), native(&tmp)) {
            Ok(()) => temps[i] = Some(tmp),
            Err(e) => r.error = Some(e.to_string()),
        }
    }
    for (i, r) in out.iter_mut().enumerate() {
        let Some(tmp) = &temps[i] else { continue };
        if let Err(e) = std::fs::rename(native(tmp), native(&r.to)) {
            // Put it back under its old name rather than leave the temporary one.
            let _ = std::fs::rename(native(tmp), native(&r.from));
            r.error = Some(e.to_string());
        }
    }
    for r in &mut out {
        if r.error.is_some() {
            r.to = String::new();
        }
    }
    out
}

/// "New folder", or Explorer's "New folder (2)", "(3)"... when taken.
pub fn create_folder(dir: &str) -> Result<String, String> {
    let mut name = "New folder".to_string();
    let mut n = 2;
    while paths::attributes(&paths::join(dir, &name)).is_ok() {
        name = format!("New folder ({n})");
        n += 1;
    }
    std::fs::create_dir(paths::to_native(&paths::join(dir, &name))).map_err(|e| e.to_string())?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hresults_map_to_states() {
        assert_eq!(classify(HRESULT(0)).0, State::Done);
        assert_eq!(classify(HRESULT(COPYENGINE_S_USER_IGNORED)).0, State::Skipped);
        assert_eq!(classify(HRESULT(COPYENGINE_E_USER_CANCELLED)).0, State::Unattempted);
        let (s, e) = classify(HRESULT(0x8007_0005u32 as i32));
        assert_eq!(s, State::Failed);
        assert!(e.is_some());
    }

    #[test]
    fn batch_rename_swaps_and_refuses_outside_collisions() {
        let d = fixture("many");
        for n in ["a.txt", "b.txt", "keep.txt"] {
            std::fs::write(paths::join(&d, n), n).unwrap();
        }
        let p = |n: &str| paths::join(&d, n);
        let r = rename_many(&[
            (p("a.txt"), "b.txt".into()),
            (p("b.txt"), "a.txt".into()),
            (p("keep.txt"), "a.txt".into()),
        ]);
        assert!(r[0].error.is_none() && r[1].error.is_none(), "{r:?}");
        assert!(r[2].error.as_deref().unwrap_or("").contains("both"), "{r:?}");
        assert_eq!(std::fs::read_to_string(p("a.txt")).unwrap(), "b.txt");
        assert_eq!(std::fs::read_to_string(p("b.txt")).unwrap(), "a.txt");
        assert_eq!(std::fs::read_to_string(p("keep.txt")).unwrap(), "keep.txt");
        let r = rename_many(&[(p("keep.txt"), "a.txt".into())]);
        assert!(r[0].error.as_deref().unwrap_or("").contains("already"), "{r:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pending_conflicts_wait_for_the_final_answer() {
        let mut log = Log { top: [r"c:\s\c2.txt".to_string()].into_iter().collect(), ..Default::default() };
        let src = || r"C:\s\c2.txt".to_string();
        log.post(src(), String::new(), HRESULT(COPYENGINE_S_PENDING), None);
        assert!(log.items.is_empty(), "a queued conflict is not an outcome");
        log.post(src(), String::new(), HRESULT(COPYENGINE_S_USER_IGNORED), None);
        assert_eq!(log.items.len(), 1);
        assert_eq!(log.items[0].state, State::Skipped);
        log.post(src(), r"C:\d\c2.txt".into(), HRESULT(0x0027_0008), None);
        assert_eq!(log.items.len(), 1, "the last report replaces earlier ones");
        assert_eq!(log.items[0].state, State::Done);
        log.post(r"C:\s\child.txt".into(), String::new(), HRESULT(0), None);
        assert_eq!((log.items.len(), log.children), (1, 1), "children are counted, not listed");
    }

    fn fixture(tag: &str) -> String {
        let d = std::env::temp_dir().join(format!("filezy-ops-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.to_string_lossy().to_string()
    }

    #[test]
    fn copy_reports_each_item_and_its_destination() {
        let d = fixture("copy");
        let (src, dst) = (paths::join(&d, "src"), paths::join(&d, "dst"));
        std::fs::create_dir_all(paths::join(&src, "folder")).unwrap();
        std::fs::write(paths::join(&src, "a.txt"), "a").unwrap();
        std::fs::write(paths::join(&src, "folder\\inner.txt"), "i").unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        let srcs = vec![paths::join(&src, "a.txt"), paths::join(&src, "folder"), paths::join(&src, "missing.txt")];
        let r = run(Op::Copy { srcs, dest: dst.clone() }, 0);
        let state = |n: &str| r.items.iter().find(|i| i.src.ends_with(n)).map(|i| i.state.clone());
        assert_eq!(state("a.txt"), Some(State::Done), "{r:?}");
        assert_eq!(state("folder"), Some(State::Done), "{r:?}");
        assert_eq!(state("missing.txt"), Some(State::Failed), "{r:?}");
        let a = r.items.iter().find(|i| i.src.ends_with("a.txt")).unwrap();
        assert!(a.dst.eq_ignore_ascii_case(&paths::join(&dst, "a.txt")));
        assert!(!a.replaced);
        assert_eq!(a.size, 1);
        assert!(std::path::Path::new(&paths::join(&dst, "folder\\inner.txt")).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn same_folder_copy_gets_a_new_name() {
        let d = fixture("dup");
        std::fs::write(paths::join(&d, "x.txt"), "x").unwrap();
        let r = run(Op::Copy { srcs: vec![paths::join(&d, "x.txt")], dest: d.clone() }, 0);
        let it = &r.items[0];
        assert_eq!(it.state, State::Done, "{r:?}");
        assert!(it.dst.ends_with("x - Copy.txt"), "{}", it.dst);
        assert!(!it.replaced);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn move_to_puts_items_back_across_names() {
        let d = fixture("moveto");
        std::fs::create_dir_all(paths::join(&d, "b")).unwrap();
        std::fs::write(paths::join(&d, "b\\renamed.txt"), "r").unwrap();
        let back = paths::join(&d, "original.txt");
        let r = run(Op::MoveTo { items: vec![(paths::join(&d, "b\\renamed.txt"), back.clone())] }, 0);
        assert_eq!(r.items[0].state, State::Done, "{r:?}");
        assert!(std::path::Path::new(&back).exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
