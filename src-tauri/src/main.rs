#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod archive;
mod caption;
mod clip;
mod cloud;
mod com;
mod drives;
mod droptarget;
mod e2edrag;
mod fs_list;
mod icons;
mod index;
mod library;
mod links;
mod listing;
mod mft;
mod newmenu;
mod ops;
mod paths;
mod pick;
mod pins;
mod preview;
mod progress;
mod props;
mod query;
mod recent;
mod recycle;
mod search;
mod session;
mod share;
mod shellmenu;
mod shellreg;
mod shellplaces;
mod tools;
mod watch;

use listing::{Entry, Group, Keep, Kept, Listing};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use ops::{Op, OpResult, State as ItemState};
use tauri::{AppHandle, Manager, State};

#[derive(Default)]
pub struct AppState {
    pub listing: Mutex<Listing>,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

/// Highest listing request (navigation, refresh, tab switch, search) the
/// frontend has started. Work for an older request stops early and never
/// replaces the listing, however late it finishes.
static LATEST: AtomicU64 = AtomicU64::new(0);

pub fn superseded(seq: u64) -> bool {
    LATEST.load(Ordering::SeqCst) > seq
}

/// Where a freshly loaded page starts numbering its requests. A reload (or a
/// recovered renderer) restarts the page's own counter, so each page load jumps
/// 2^32 ahead: every request of an earlier page, even one still on its way,
/// then counts as superseded, and the new page's never do.
#[tauri::command]
fn seq_base() -> u64 {
    const PAGE: u64 = 1 << 32;
    LATEST.fetch_add(PAGE, Ordering::SeqCst) + PAGE
}

pub const STALE: &str = "The folder changed before the command ran. Check the selection and try again.";

/// How a listing is ordered: the sort and the grouping applied after it.
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Arrangement {
    pub sort: String,
    pub asc: bool,
    pub group: String,
    /// Minutes to add to UTC for local time, so date groups fall on the user's days.
    pub tz_off_min: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenReq {
    pub path: String,
    pub seq: u64,
    pub arr: Arrangement,
    pub show_hidden: bool,
    pub show_protected: bool,
    #[serde(default)]
    pub keep: Keep,
}

#[derive(Serialize)]
pub struct Opened {
    path: String,
    rev: u64,
    total: usize,
    groups: Vec<Group>,
    kept: Kept,
    /// False when a newer request started first; nothing was replaced.
    installed: bool,
    /// False when Windows can't report changes here, so the view won't update itself.
    live: bool,
    /// Something the user should know that didn't stop the listing.
    notice: Option<String>,
    /// The folder is in one a cloud provider syncs, so its items have a Status.
    cloud: bool,
}

/// Arrange `l`, falling back to name order (and saying so) when the requested
/// property sort is too large.
fn arrange_or_name(l: &mut Listing, arr: &Arrangement) -> (Vec<Group>, Option<String>) {
    let tz = arr.tz_off_min * 60_000;
    match l.arrange(&arr.sort, arr.asc, &arr.group, tz) {
        Ok(g) => (g, None),
        Err(e) => (l.arrange("name", true, &arr.group, tz).unwrap_or_default(), Some(e)),
    }
}

#[tauri::command]
async fn open_dir(req: OpenReq, state: State<'_, AppState>, app: AppHandle) -> Result<Opened, String> {
    LATEST.fetch_max(req.seq, Ordering::SeqCst);
    let seq = req.seq;
    let (path, inside, library) = match paths::resolve(&req.path)? {
        paths::Target::Dir(p) => (p, None, false),
        paths::Target::File(p) => return Err(format!("'{p}' is a file, not a folder.")),
        paths::Target::Archive(file, inner) => (archive::location(&file, &inner), Some((file, inner)), false),
        paths::Target::Library(file) => (file, None, true),
    };
    let folder = inside.is_none() && !library;
    let built = tauri::async_runtime::spawn_blocking({
        let path = path.clone();
        let arr = req.arr.clone();
        // None: a newer request superseded this one. The string is a notice for the status bar.
        type Built = Option<(Listing, Vec<Group>, Option<String>)>;
        move || -> Result<Built, String> {
            let mut entries = match &inside {
                Some((file, inner)) => archive::list(file, inner)?,
                // A library: its folders' items together, each under its real folder.
                None if library => {
                    let mut all = Vec::new();
                    for dir in library::folders(&path)? {
                        let Some(mut entries) = fs_list::list_with(&dir, || !superseded(seq))? else { return Ok(None) };
                        entries.iter_mut().for_each(|e| e.parent = Some(dir.clone()));
                        all.append(&mut entries);
                    }
                    all
                }
                None => {
                    let Some(entries) = fs_list::list_with(&path, || !superseded(seq))? else { return Ok(None) };
                    entries
                }
            };
            entries.retain(|e| listing::visible(e.attrs, req.show_hidden, req.show_protected));
            let mut l = Listing { path, entries, ..Default::default() };
            if superseded(seq) {
                return Ok(None);
            }
            let (groups, notice) = arrange_or_name(&mut l, &arr);
            Ok(Some((l, groups, notice)))
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    let not_installed = |path: String| Opened {
        path,
        rev: 0,
        total: 0,
        groups: vec![],
        kept: Kept::default(),
        installed: false,
        live: false,
        notice: None,
        cloud: false,
    };
    let Some((l, groups, notice)) = built else { return Ok(not_installed(path)) };
    let cloud = folder && cloud::synced(&path);
    let (rev, total, kept) = {
        let mut cur = state.listing.lock().unwrap();
        // Checked under the lock: a newer request may have installed its listing meanwhile.
        if superseded(seq) {
            return Ok(not_installed(path));
        }
        let keys = cur.keep_keys(&req.keep);
        let kept = l.find(&keys, &req.keep.names);
        let (rev, total) = (l.rev, l.len());
        *cur = l;
        (rev, total, kept)
    };
    let live = watch::watch(&app, &state, &path);
    Ok(Opened { path, rev, total, groups, kept, installed: true, live, notice, cloud })
}

/// A virtual page took over the tab: stop loads and searches started before `seq`.
#[tauri::command]
fn cancel_listing(seq: u64) {
    LATEST.fetch_max(seq, Ordering::SeqCst);
}

#[tauri::command]
fn get_range(rev: u64, start: usize, count: usize, state: State<'_, AppState>) -> Result<Vec<Entry>, String> {
    let l = state.listing.lock().unwrap();
    if l.rev != rev {
        return Err(STALE.into());
    }
    Ok(l.entries.iter().skip(start).take(count).cloned().collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArrangeReq {
    rev: u64,
    arr: Arrangement,
    #[serde(default)]
    keep: Keep,
}

#[derive(Serialize)]
struct Arranged {
    rev: u64,
    total: usize,
    groups: Vec<Group>,
    kept: Kept,
    notice: Option<String>,
}

/// Re-sort and regroup the current listing. Runs on a copy, so a slow property
/// sort doesn't hold the listing lock; search results streamed in meanwhile are
/// kept at the end.
#[tauri::command]
async fn arrange(req: ArrangeReq, state: State<'_, AppState>) -> Result<Arranged, String> {
    let (mut copy, keys) = {
        let l = state.listing.lock().unwrap();
        if l.rev != req.rev {
            return Err(STALE.into());
        }
        let copy = Listing { rev: l.rev, path: l.path.clone(), entries: l.entries.clone(), ..Default::default() };
        (copy, l.keep_keys(&req.keep))
    };
    let arr = req.arr.clone();
    let (copy, groups, notice) = tauri::async_runtime::spawn_blocking(move || {
        let (groups, notice) = arrange_or_name(&mut copy, &arr);
        (copy, groups, notice)
    })
    .await
    .map_err(|e| e.to_string())?;
    let mut l = state.listing.lock().unwrap();
    if l.rev != req.rev {
        return Err(STALE.into());
    }
    let mut copy = copy;
    let sorted = copy.entries.len();
    copy.entries.extend(l.entries.drain(sorted..));
    let kept = copy.find(&keys, &req.keep.names);
    let (rev, total) = (copy.rev, copy.len());
    l.entries = copy.entries;
    l.rev = rev;
    l.sort = copy.sort;
    l.asc = copy.asc;
    l.group = copy.group;
    Ok(Arranged { rev, total, groups, kept, notice })
}

#[derive(Serialize)]
struct Resolved {
    kind: &'static str,
    path: String,
}

/// What the address bar's text points at, so a typed file path opens the file.
/// Off the UI thread: a share that's gone takes Windows a while to give up on.
#[tauri::command]
async fn resolve_path(input: String) -> Result<Resolved, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(match paths::resolve(&input)? {
            paths::Target::Dir(path) => Resolved { kind: "dir", path },
            paths::Target::File(path) => Resolved { kind: "file", path },
            paths::Target::Archive(file, inner) => Resolved { kind: "dir", path: archive::location(&file, &inner) },
            paths::Target::Library(path) => Resolved { kind: "dir", path },
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Display paths of the rows a selection names, resolved against the exact
/// listing revision the frontend saw.
pub fn sel_paths(state: &AppState, sel: &Keep) -> Result<Vec<String>, String> {
    let l = state.listing.lock().unwrap();
    if sel.rev != l.rev {
        return Err(STALE.into());
    }
    let dir = l.path.clone();
    let pick = |e: &Entry| e.path_in(&dir);
    Ok(if sel.all {
        l.entries.iter().map(pick).collect()
    } else {
        sel.idx.iter().filter_map(|&i| l.entries.get(i as usize)).map(pick).collect()
    })
}

#[derive(Serialize)]
struct SelStats {
    count: usize,
    files: usize,
    bytes: u64,
    /// Oldest and newest modified times among the items (0 when none).
    oldest: u64,
    newest: u64,
}

#[tauri::command]
fn selection_stats(sel: Keep, state: State<'_, AppState>) -> Result<SelStats, String> {
    let l = state.listing.lock().unwrap();
    if sel.rev != l.rev {
        return Err(STALE.into());
    }
    let mut s = SelStats { count: 0, files: 0, bytes: 0, oldest: 0, newest: 0 };
    let mut add = |e: &Entry| {
        s.count += 1;
        if e.modified > 0 {
            s.oldest = if s.oldest == 0 { e.modified } else { s.oldest.min(e.modified) };
            s.newest = s.newest.max(e.modified);
        }
        if !e.is_dir {
            s.files += 1;
            s.bytes += e.size;
        }
    };
    if sel.all {
        l.entries.iter().for_each(&mut add);
    } else {
        sel.idx.iter().filter_map(|&i| l.entries.get(i as usize)).for_each(&mut add);
    }
    Ok(s)
}

/// Current indices of rows by identity key (see Entry::key); -1 when gone.
#[tauri::command]
fn find_keys(rev: u64, keys: Vec<String>, state: State<'_, AppState>) -> Result<Vec<i64>, String> {
    let l = state.listing.lock().unwrap();
    if l.rev != rev {
        return Err(STALE.into());
    }
    Ok(l.find(&listing::KeepKeys::default(), &keys).names)
}

/// Type-to-select: the first row at or after `from` (wrapping) whose name starts
/// with `prefix`, ignoring case, in the listing revision the frontend sees.
#[tauri::command]
fn find_prefix(rev: u64, prefix: String, from: usize, state: State<'_, AppState>) -> Result<Option<usize>, String> {
    let l = state.listing.lock().unwrap();
    if l.rev != rev {
        return Err(STALE.into());
    }
    let p = prefix.to_lowercase();
    let n = l.entries.len();
    Ok((0..n).map(|k| (from + k) % n).find(|&i| l.entries[i].name.to_lowercase().starts_with(&p)))
}

#[derive(Serialize)]
struct SelItem {
    path: String,
    is_dir: bool,
}

/// Paths and kinds of every selected row (fetched or not), for Open on a selection.
#[tauri::command]
fn sel_items(sel: Keep, state: State<'_, AppState>) -> Result<Vec<SelItem>, String> {
    let l = state.listing.lock().unwrap();
    if sel.rev != l.rev {
        return Err(STALE.into());
    }
    let item = |e: &Entry| SelItem { path: e.path_in(&l.path), is_dir: e.is_dir };
    Ok(if sel.all {
        l.entries.iter().map(item).collect()
    } else {
        sel.idx.iter().filter_map(|&i| l.entries.get(i as usize)).map(item).collect()
    })
}

/// Identity keys (see Entry::key) of selected rows, so a tab can restore its
/// selection after another tab replaced the listing.
#[tauri::command]
fn sel_keys(sel: Keep, state: State<'_, AppState>) -> Vec<String> {
    state.listing.lock().unwrap().sel_keys(&sel)
}

#[tauri::command]
fn clip_copy(sel: Keep, cut: bool, state: State<'_, AppState>) -> Result<(), String> {
    let paths = sel_paths(&state, &sel)?;
    clip::set_files(&paths, cut)
}

#[derive(Serialize)]
struct ClipState {
    /// Files are on the clipboard, so Paste can work.
    files: bool,
    /// The files, when they were cut, so the list can dim them like Explorer.
    cut: Vec<String>,
}

#[tauri::command]
async fn clip_state() -> ClipState {
    tauri::async_runtime::spawn_blocking(|| match clip::get_files() {
        Some((paths, true)) => ClipState { files: true, cut: paths },
        Some(_) => ClipState { files: true, cut: vec![] },
        None => ClipState { files: clip::has_files(), cut: vec![] },
    })
    .await
    .unwrap_or(ClipState { files: false, cut: vec![] })
}

/// The shell's menu for the selection, every selected item included.
#[tauri::command]
async fn shell_menu(sel: Keep, state: State<'_, AppState>, menu: State<'_, shellmenu::ShellMenu>) -> Result<shellmenu::Menu, String> {
    let paths = sel_paths(&state, &sel)?;
    let menu = menu.inner().clone();
    tauri::async_runtime::spawn_blocking(move || menu.build(paths)).await.map_err(|e| e.to_string())?
}

/// The shell's menu for explicit items (drive tiles).
#[tauri::command]
async fn shell_menu_paths(paths: Vec<String>, menu: State<'_, shellmenu::ShellMenu>) -> Result<shellmenu::Menu, String> {
    let menu = menu.inner().clone();
    tauri::async_runtime::spawn_blocking(move || menu.build(paths)).await.map_err(|e| e.to_string())?
}

/// The menu `session` read again, with entries extensions added since it was built.
#[tauri::command]
async fn shell_menu_items(session: u64, menu: State<'_, shellmenu::ShellMenu>) -> Result<Vec<shellmenu::Item>, String> {
    let menu = menu.inner().clone();
    tauri::async_runtime::spawn_blocking(move || menu.reread(session)).await.map_err(|e| e.to_string())?
}

/// Run a command from the menu `session`; a command from an older menu is refused.
#[tauri::command]
async fn shell_menu_invoke(session: u64, id: u32, menu: State<'_, shellmenu::ShellMenu>) -> Result<(), String> {
    let menu = menu.inner().clone();
    tauri::async_runtime::spawn_blocking(move || menu.invoke(session, id)).await.map_err(|e| e.to_string())?
}

/// Windows' own context menu ("Show more options") at a point in the window,
/// in physical pixels. A picked verb listed in `handled` comes back for the
/// page to run; anything else runs here.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn shell_menu_native(
    sel: Option<Keep>,
    paths: Option<Vec<String>>,
    x: i32,
    y: i32,
    extended: bool,
    handled: Vec<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Option<String>, String> {
    let paths = match (sel, paths) {
        (Some(sel), _) => sel_paths(&state, &sel)?,
        (None, Some(p)) => p,
        (None, None) => return Ok(None),
    };
    if paths.len() > shellmenu::menu_max() {
        return Err(format!("Shell options aren't shown for more than {} items.", shellmenu::menu_max()));
    }
    let owner = owner(&app);
    on_main(&app, move || shellmenu::native(&paths, owner, x, y, extended, &handled)).await?
}

/// Run `f` on the UI thread and wait for its result without holding an async worker.
async fn on_main<T: Send + 'static>(app: &AppHandle, f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn copy_as_path(sel: Keep, state: State<'_, AppState>) -> Result<(), String> {
    let paths = sel_paths(&state, &sel)?;
    let text = paths.iter().map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join("\r\n");
    clip::set_text(&text)
}

/// Properties dialog for an explicit path (drive tiles, nav items).
#[tauri::command]
async fn show_properties(path: String, app: AppHandle) -> Result<(), String> {
    on_main(&app, move || shellmenu::properties(&[path])).await?
}

/// Properties for the selection: one item's sheet, or Windows' combined sheet.
#[tauri::command]
async fn sel_properties(sel: Keep, state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let paths = sel_paths(&state, &sel)?;
    on_main(&app, move || shellmenu::properties(&paths)).await?
}

/// Windows' Share sheet for the selected files.
#[tauri::command]
async fn share(sel: Keep, state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let paths = sel_paths(&state, &sel)?;
    let files = tauri::async_runtime::spawn_blocking(move || share::files(&paths)).await.map_err(|e| e.to_string())??;
    // The sheet belongs to the window's UI thread.
    let owner = owner(&app);
    on_main(&app, move || share::show(owner, files)).await?
}

/// Property values for the visible rows: one list per (path, modified) item,
/// in the order of `keys`.
#[tauri::command]
async fn props_for(items: Vec<(String, u64)>, keys: Vec<String>) -> Vec<Vec<Option<String>>> {
    tauri::async_runtime::spawn_blocking(move || items.iter().map(|(p, m)| props::values(p, *m, &keys)).collect())
        .await
        .unwrap_or_default()
}

/// The properties this file lets a person change (Details pane).
#[tauri::command]
async fn props_writable(path: String) -> Vec<String> {
    tauri::async_runtime::spawn_blocking(move || props::writable(&path)).await.unwrap_or_default()
}

/// Set a property on every selected item; each reports its own result.
#[tauri::command]
async fn props_write(sel: Keep, key: String, value: String, state: State<'_, AppState>) -> Result<Vec<props::Written>, String> {
    let paths = sel_paths(&state, &sel)?;
    tauri::async_runtime::spawn_blocking(move || props::write(&paths, &key, &value)).await.map_err(|e| e.to_string())
}

/// A shortcut's target, for the Details pane.
#[tauri::command]
async fn link_target(path: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || links::target(&path)).await.ok().flatten()
}

#[derive(Serialize)]
struct LocaleFormats {
    date: String,
    time: String,
    am: String,
    pm: String,
}

/// The user's short date and time formats from Windows ("dd-MM-yyyy",
/// "HH:mm"), so dates read exactly as in Explorer rather than in the
/// WebView's idea of the locale.
#[tauri::command]
fn locale_formats() -> LocaleFormats {
    use windows::core::PCWSTR;
    use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_S1159, LOCALE_S2359, LOCALE_SSHORTDATE, LOCALE_SSHORTTIME};
    let get = |t: u32| unsafe {
        let mut buf = [0u16; 128];
        let n = GetLocaleInfoEx(PCWSTR::null(), t, Some(&mut buf));
        String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize])
    };
    LocaleFormats { date: get(LOCALE_SSHORTDATE), time: get(LOCALE_SSHORTTIME), am: get(LOCALE_S1159), pm: get(LOCALE_S2359) }
}

/// Batch rename: (path, new name) pairs, applied together.
#[tauri::command]
async fn rename_many(items: Vec<(String, String)>) -> Result<Vec<ops::Renamed>, String> {
    tauri::async_runtime::spawn_blocking(move || ops::rename_many(&items)).await.map_err(|e| e.to_string())
}

/// Create shortcut: one per selected item, next to it.
#[tauri::command]
async fn create_shortcuts(sel: Keep, state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let paths = sel_paths(&state, &sel)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut made = Vec::new();
        for p in &paths {
            let dir = paths::parent(p).ok_or("A shortcut to a drive needs a folder to go in; use Paste shortcut.")?;
            made.extend(links::create(std::slice::from_ref(p), &dir)?);
        }
        Ok(made)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Paste shortcut: shortcuts in `dest` to whatever files are on the clipboard.
#[tauri::command]
async fn paste_shortcuts(dest: String) -> Result<Vec<String>, String> {
    let Some((srcs, _)) = clip::get_files() else { return Ok(vec![]) };
    let dest = folder(&dest)?;
    tauri::async_runtime::spawn_blocking(move || links::create(&srcs, &dest)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn start_drag(sel: Keep, state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let paths = sel_paths(&state, &sel)?;
    if paths.is_empty() {
        return Ok(());
    }
    if let Some((script, done)) = e2edrag::take_armed() {
        tauri::async_runtime::spawn_blocking(move || {
            let _ = done.send(e2edrag::run(&app, paths, script));
        });
        return Ok(());
    }
    // The drag loop must run on the UI thread (see shellmenu::drag).
    let owner = owner(&app);
    app.run_on_main_thread(move || {
        let _ = shellmenu::drag(&paths, owner);
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_with(path: String) -> Result<(), String> {
    ops::shell_open_verb(&path, "openas")
}

/// Rename in place. A name that differs only in case is a real rename; any
/// other existing item with the new name is a collision.
#[tauri::command]
async fn rename_item(path: String, new_name: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || rename_now(&path, &new_name)).await.map_err(|e| e.to_string())?
}

fn rename_now(path: &str, new_name: &str) -> Result<String, String> {
    let name = paths::check_name(new_name)?;
    let parent = paths::parent(path).ok_or("This item can't be renamed.")?;
    let old_name = paths::file_name(path);
    if name == old_name {
        return Ok(path.to_string());
    }
    let to = paths::join(&parent, &name);
    if paths::identity(&to) != paths::identity(path) && paths::attributes(&to).is_ok() {
        return Err(format!("There is already a file with the name '{name}' in this location."));
    }
    std::fs::rename(paths::to_native(path), paths::to_native(&to)).map_err(|e| e.to_string())?;
    Ok(to)
}

#[tauri::command]
/// Make "New folder" in `dir`; returns its path. A library's goes to its save location.
async fn create_folder(dir: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = folder(&dir)?;
        ops::create_folder(&dir).map(|name| paths::join(&dir, &name))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn new_menu_items() -> Vec<newmenu::NewItem> {
    // First call scans HKCR (~100ms); cached afterwards.
    tauri::async_runtime::spawn_blocking(newmenu::items).await.unwrap_or_default()
}

#[tauri::command]
/// Make a new file of a type from the New menu in `dir`; returns its path, or
/// None when the type launched a wizard instead. A library's goes to its save location.
async fn create_new(dir: String, ext: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = folder(&dir)?;
        newmenu::create(&dir, &ext).map(|name| name.map(|n| paths::join(&dir, &n)))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Open an item as double-clicking it does. Returns a location to show
/// instead when the item is an archive to look inside (Windows' zip folder is
/// its program, or it sits inside another archive). A file inside an archive
/// is copied out first and that copy opened.
#[tauri::command]
async fn open_path(path: String, app: AppHandle) -> Result<Option<String>, String> {
    let target = match archive::split(&path) {
        Some((file, inner)) => {
            let (file, inner, owner) = (file.to_string(), inner.to_string(), owner(&app));
            let title = format!("Opening {}", paths::file_name(&inner));
            let line = format!("From {}", paths::file_name(&file));
            let staged = tauri::async_runtime::spawn_blocking(move || {
                progress::run(move || {
                    let mut p = progress::Progress::new(owner, &title, &line);
                    archive::stage(&file, &inner, &mut |done, all, name| p.tick(done, all, name))
                })
            })
            .await
            .map_err(|e| e.to_string())???;
            if archive::kind_of(&staged).is_some() {
                return Ok(Some(archive::location(&staged, "")));
            }
            staged
        }
        None if archive::browses_inside(&path) => return Ok(Some(archive::location(&path, ""))),
        None if library::is_library(&path) => return Ok(Some(path)),
        None => path,
    };
    on_main(&app, move || ops::shell_open(&target)).await??;
    Ok(None)
}

#[derive(Deserialize)]
struct ExtractReq {
    /// The archive.
    file: String,
    /// The folder inside it the items were picked in ("" for its root).
    base: String,
    /// Inner paths of the picked items; [""] for everything.
    picked: Vec<String>,
    dest: String,
}

/// Extract from an archive into a folder, with Windows' progress dialog.
#[tauri::command]
async fn archive_extract(req: ExtractReq, app: AppHandle) -> Result<archive::Extracted, String> {
    let owner = owner(&app);
    tauri::async_runtime::spawn_blocking(move || {
        progress::run(move || {
            let title = format!("Extracting {}", paths::file_name(&req.file));
            let mut p = progress::Progress::new(owner, &title, &format!("To {}", req.dest));
            archive::extract_to(&req.file, &req.base, &req.picked, &req.dest, owner, &mut |d, a, n| p.tick(d, a, n))
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Compress the selection into a new ZIP beside it, named after `name_from`.
#[tauri::command]
async fn archive_compress(sel: Keep, name_from: String, state: State<'_, AppState>, app: AppHandle) -> Result<archive::Compressed, String> {
    let items = sel_paths(&state, &sel)?;
    let owner = owner(&app);
    tauri::async_runtime::spawn_blocking(move || {
        progress::run(move || {
            let mut p = progress::Progress::new(owner, "Compressing", &format!("{} to a ZIP file", name_from));
            archive::compress_to(&items, &name_from, &mut |d, a, n| p.tick(d, a, n))
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Windows' folder picker, starting in `start`; None when cancelled.
#[tauri::command]
async fn pick_folder(start: String, title: String, app: AppHandle) -> Result<Option<String>, String> {
    let owner = owner(&app);
    on_main(&app, move || pick::folder(owner, &start, &title)).await?
}

/// Real shell icon for an arbitrary path (nav pane, Home tiles, drive tiles).
#[tauri::command]
async fn path_thumb(path: String, size: i32) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || icons::shell_icon_png(&path, size))
        .await
        .unwrap_or(None)
}

/// Quick access pins, or with `kind` "favorites" the Favorites files (pins.rs).
#[tauri::command]
fn get_pins(kind: Option<String>) -> Vec<String> {
    pins::load(kind.as_deref().unwrap_or("pins"))
}

#[tauri::command]
fn set_pins(pins: Vec<String>, kind: Option<String>) -> Result<(), String> {
    pins::save(kind.as_deref().unwrap_or("pins"), &pins)
}

#[tauri::command]
async fn recent_files() -> Vec<recent::RecentFile> {
    tauri::async_runtime::spawn_blocking(|| recent::list(20)).await.unwrap_or_default()
}

/// Remove files from Recent files (their shortcuts in Windows' Recent items); returns how many shortcuts went.
#[tauri::command]
async fn recent_remove(paths: Vec<String>) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || recent::remove(&paths)).await.map_err(|e| e.to_string())?
}

/// Clear Recent files: every shortcut in Windows' Recent items, nothing else.
#[tauri::command]
async fn recent_clear() -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(recent::clear).await.map_err(|e| e.to_string())?
}

/// How the preview pane shows a file (preview.rs).
#[tauri::command]
async fn preview_info(path: String, download: Option<bool>, app: AppHandle) -> Result<preview::Info, String> {
    tauri::async_runtime::spawn_blocking(move || preview::info(&app, &path, download.unwrap_or(false))).await.map_err(|e| e.to_string())?
}

/// The start of a text file, decoded; None when it isn't text.
#[tauri::command]
async fn preview_text(path: String) -> Result<Option<preview::Text>, String> {
    tauri::async_runtime::spawn_blocking(move || preview::text(&path)).await.map_err(|e| e.to_string())?
}

/// Show a file with Windows' preview handler for its type. `colors` are the
/// pane's background and text as COLORREFs.
#[tauri::command]
async fn preview_native_open(path: String, place: preview::Place, colors: Option<(u32, u32)>, app: AppHandle) -> Result<bool, String> {
    preview::open(app, path, place, colors).await
}

#[tauri::command]
fn preview_native_place(place: Option<preview::Place>, app: AppHandle) {
    preview::place(&app, place)
}

#[tauri::command]
fn preview_native_close(app: AppHandle) {
    preview::close(&app)
}

#[tauri::command]
fn ext_meta(ext: String, is_dir: bool) -> icons::ExtMeta {
    icons::ext_meta(&ext, is_dir)
}

#[tauri::command]
async fn path_icon(path: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || icons::path_icon(&path)).await.unwrap_or(None)
}

/// Thumbnail or large icon for a file, by path, so a late answer can only ever
/// land on the item it was asked for.
#[tauri::command]
async fn thumb(path: String, size: i32) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || icons::thumbnail(&path, size))
        .await
        .unwrap_or(None)
}

/// Every drive, each with its state. Off the UI thread: a drive that doesn't
/// answer takes up to drives::PATIENCE to be given up on.
#[tauri::command]
async fn drives() -> Vec<drives::Drive> {
    tauri::async_runtime::spawn_blocking(drives::list).await.unwrap_or_default()
}

/// The computers Windows finds on the network (Network page).
#[tauri::command]
async fn network_places() -> Vec<drives::Place> {
    tauri::async_runtime::spawn_blocking(|| drives::network(std::time::Duration::from_secs(6))).await.unwrap_or_default()
}

/// Windows' Map Network Drive dialog, with its own sign-in prompts. True when
/// a drive was mapped.
#[tauri::command]
async fn map_drive(app: AppHandle) -> Result<bool, String> {
    use windows::Win32::Foundation::NO_ERROR;
    use windows::Win32::NetworkManagement::WNet::{WNetConnectionDialog, RESOURCETYPE_DISK};
    let owner = owner(&app);
    on_main(&app, move || unsafe { WNetConnectionDialog(windows::Win32::Foundation::HWND(owner as *mut _), RESOURCETYPE_DISK.0) == NO_ERROR }).await
}

/// Disconnect a network drive ("Z:\"), or with no drive, Windows' Disconnect
/// Network Drives dialog to pick one.
#[tauri::command]
async fn disconnect_drive(path: Option<String>, app: AppHandle) -> Result<(), String> {
    use windows::Win32::Foundation::{ERROR_OPEN_FILES, NO_ERROR};
    use windows::Win32::NetworkManagement::WNet::{WNetCancelConnection2W, WNetDisconnectDialog, CONNECT_UPDATE_PROFILE, RESOURCETYPE_DISK};
    let owner = owner(&app);
    match path {
        None => {
            on_main(&app, move || unsafe { let _ = WNetDisconnectDialog(Some(windows::Win32::Foundation::HWND(owner as *mut _)), RESOURCETYPE_DISK.0); }).await?;
            Ok(())
        }
        Some(p) => {
            let name = fs_list::wide(p.trim_end_matches('\\'));
            match unsafe { WNetCancelConnection2W(windows::core::PCWSTR(name.as_ptr()), CONNECT_UPDATE_PROFILE, false) } {
                NO_ERROR => Ok(()),
                ERROR_OPEN_FILES => Err(format!("Files or folders on {} are open. Close them and try again.", p.trim_end_matches('\\'))),
                e => Err(windows::core::Error::from(e.to_hresult()).message().to_string()),
            }
        }
    }
}

/// Eject a disc, card or USB drive, as Explorer's Eject does (its shell verb,
/// which also says when the drive is safe to remove).
#[tauri::command]
async fn eject(path: String, app: AppHandle) -> Result<(), String> {
    let owner = owner(&app);
    tauri::async_runtime::spawn_blocking(move || progress::run(move || shellmenu::run_verb(&[path], "eject", owner)))
        .await
        .map_err(|e| e.to_string())??
}

#[tauri::command]
fn special_dirs() -> Vec<(String, String)> {
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music,
        FOLDERID_Pictures, FOLDERID_Videos, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
    };
    let profile = std::env::var("USERPROFILE").unwrap_or_default();
    // Real known-folder paths (OneDrive redirection etc.), USERPROFILE join fallback.
    [
        ("Desktop", &FOLDERID_Desktop),
        ("Documents", &FOLDERID_Documents),
        ("Downloads", &FOLDERID_Downloads),
        ("Pictures", &FOLDERID_Pictures),
        ("Music", &FOLDERID_Music),
        ("Videos", &FOLDERID_Videos),
    ]
    .iter()
    .map(|(n, id)| {
        let path = unsafe { SHGetKnownFolderPath(*id, KF_FLAG_DEFAULT, None) }
            .ok()
            .and_then(|p| unsafe { p.to_string() }.ok())
            .unwrap_or_else(|| format!("{profile}\\{n}"));
        (n.to_string(), path)
    })
    .filter(|(_, p)| std::path::Path::new(p).is_dir())
    .collect()
}

#[tauri::command]
fn home_dir() -> String {
    std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".into())
}

/// Cloud folders for the navigation pane, as Explorer lists them (cloud.rs).
#[tauri::command]
async fn cloud_roots() -> Vec<cloud::Root> {
    tauri::async_runtime::spawn_blocking(cloud::roots).await.unwrap_or_default()
}

/// Explorer's Status text for these items ("Available when online"...), or
/// None for items without one.
#[tauri::command]
async fn cloud_status(paths: Vec<String>) -> Vec<Option<props::CloudState>> {
    tauri::async_runtime::spawn_blocking(move || props::cloud_states(&paths)).await.unwrap_or_default()
}

/// Always keep on this device (`keep`) or Free up space, for the selection.
/// Both are Windows' own verbs: they set the pin state, and the provider
/// downloads or frees the files. Keeping is a toggle, as in Explorer: on
/// items that are already kept, it stops keeping them.
#[tauri::command]
async fn cloud_pin(sel: Keep, keep: bool, state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let paths = sel_paths(&state, &sel)?;
    let owner = owner(&app);
    let verb = if keep { "MakeAvailableOffline" } else { "MakeAvailableOnline" };
    tauri::async_runtime::spawn_blocking(move || progress::run(move || shellmenu::run_verb(&paths, verb, owner)))
        .await
        .map_err(|e| e.to_string())??
}

/// The main window, which the native file-operation dialogs belong to.
fn owner(app: &AppHandle) -> isize {
    app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize).unwrap_or(0)
}

async fn run_op(op: Op, app: &AppHandle) -> Result<OpResult, String> {
    let owner = owner(app);
    tauri::async_runtime::spawn_blocking(move || ops::run(op, owner)).await.map_err(|e| e.to_string())
}

fn folder(path: &str) -> Result<String, String> {
    match paths::resolve(path)? {
        paths::Target::Dir(d) => Ok(d),
        paths::Target::File(_) => Err("Items can only go into a folder.".into()),
        paths::Target::Archive(..) => Err("Filezy can't add items to an archive yet. Extract it, or make a new ZIP with Compress to ZIP file.".into()),
        // As in Explorer, new items go to the library's default save location.
        paths::Target::Library(file) => library::save_folder(&file),
    }
}

/// Paste the clipboard's files into `dest`. A cut pastes once: afterwards the
/// clipboard keeps only the items that did not move (skipped, failed, never
/// reached), and only if nothing else was put on the clipboard meanwhile.
#[tauri::command]
async fn paste(dest: String, app: AppHandle) -> Result<OpResult, String> {
    let seq = clip::sequence();
    let Some((srcs, cut)) = clip::get_files() else {
        return Ok(OpResult { kind: "copy", items: vec![], aborted: false, cancelled: false, error: None, children: 0, clipboard: None });
    };
    let dest = folder(&dest)?;
    let op = if cut { Op::Move { srcs: srcs.clone(), dest } } else { Op::Copy { srcs: srcs.clone(), dest } };
    let mut res = run_op(op, &app).await?;
    if cut {
        res.clipboard = Some(if clip::sequence() != seq {
            "changed elsewhere"
        } else {
            let moved: std::collections::HashSet<String> =
                res.items.iter().filter(|i| i.state == ItemState::Done).map(|i| i.src.to_lowercase()).collect();
            let left: Vec<String> = srcs.into_iter().filter(|s| !moved.contains(&s.to_lowercase())).collect();
            if left.is_empty() {
                clip::clear();
                "cleared"
            } else {
                clip::set_files(&left, true)?;
                "rewritten"
            }
        });
    }
    Ok(res)
}

/// Files dropped from another app (or a drag within this one) onto `dest`.
#[tauri::command]
async fn drop_paths(paths: Vec<String>, dest: String, op: String, app: AppHandle) -> Result<OpResult, String> {
    let dest = folder(&dest)?;
    let mv = match op.as_str() {
        "move" => true,
        "copy" => false,
        _ => return Err(format!("Unknown drop operation '{op}'.")),
    };
    // Moving items onto their own folder does nothing; a copy there gets new names.
    let srcs: Vec<String> = paths
        .into_iter()
        .filter(|p| !mv || paths::parent(p).is_none_or(|parent| !parent.eq_ignore_ascii_case(&dest)))
        .collect();
    if srcs.is_empty() {
        return Ok(OpResult { kind: "move", items: vec![], aborted: false, cancelled: false, error: None, children: 0, clipboard: None });
    }
    run_op(if mv { Op::Move { srcs, dest } } else { Op::Copy { srcs, dest } }, &app).await
}

/// "Create shortcuts here" from a drop: shortcuts in `dest` to `paths`.
#[tauri::command]
async fn make_links(paths: Vec<String>, dest: String) -> Result<Vec<String>, String> {
    let dest = folder(&dest)?;
    tauri::async_runtime::spawn_blocking(move || links::create(&paths, &dest)).await.map_err(|e| e.to_string())?
}

/// Take over drag and drop for the window (see droptarget.rs). The page calls
/// this when it loads.
#[tauri::command]
fn drop_install(app: AppHandle) -> Result<(), String> {
    let top = owner(&app);
    let handle = app.clone();
    app.run_on_main_thread(move || droptarget::install(&handle, top)).map_err(|e| e.to_string())
}

/// The page's answer to a drag position: where a drop there would go.
#[tauri::command]
async fn drop_dest(dest: Option<droptarget::Dest>) {
    droptarget::set_dest(dest);
}

/// Delete the selection: to the Recycle Bin, or permanently (Shift+Delete)
/// after Windows' own confirmation.
#[tauri::command]
async fn delete_items(sel: Keep, permanent: bool, state: State<'_, AppState>, app: AppHandle) -> Result<OpResult, String> {
    let paths = sel_paths(&state, &sel)?;
    run_op(if permanent { Op::Delete { paths } } else { Op::Recycle { paths } }, &app).await
}

/// Undo of a copy or a new item: recycle what was created, so it stays recoverable.
#[tauri::command]
async fn recycle_paths(paths: Vec<String>, app: AppHandle) -> Result<OpResult, String> {
    run_op(Op::Recycle { paths }, &app).await
}

/// Undo of a move or rename: put each item back at its exact old path, through
/// the shell's engine so cross-volume moves work.
#[tauri::command]
async fn move_back(items: Vec<(String, String)>, app: AppHandle) -> Result<OpResult, String> {
    run_op(Op::MoveTo { items }, &app).await
}

/// Redo of a move: move items into a folder again.
#[tauri::command]
async fn move_into(srcs: Vec<String>, dest: String, app: AppHandle) -> Result<OpResult, String> {
    run_op(Op::Move { srcs, dest }, &app).await
}

/// Redo of a copy: copy items into a folder again.
#[tauri::command]
async fn copy_into(srcs: Vec<String>, dest: String, app: AppHandle) -> Result<OpResult, String> {
    run_op(Op::Copy { srcs, dest }, &app).await
}

#[derive(Serialize)]
struct Stamp {
    exists: bool,
    size: u64,
    modified: u64,
}

/// Current size and time of paths, so undo can see whether an item changed
/// after the operation it would reverse.
#[tauri::command]
async fn path_stamps(paths: Vec<String>) -> Vec<Stamp> {
    tauri::async_runtime::spawn_blocking(move || stamps(&paths)).await.unwrap_or_default()
}

fn stamps(paths: &[String]) -> Vec<Stamp> {
    paths
        .iter()
        .map(|p| {
            let exists = paths::attributes(p).is_ok();
            let (size, modified) = if exists { ops::stamp(p) } else { (0, 0) };
            Stamp { exists, size, modified }
        })
        .collect()
}

#[tauri::command]
async fn recycle_list() -> Result<recycle::RecycleList, String> {
    tauri::async_runtime::spawn_blocking(recycle::list).await.map_err(|e| e.to_string())?
}

/// Restore Recycle Bin items by id (see recycle.rs); returns how many came back.
#[tauri::command]
async fn recycle_restore(ids: Vec<String>, app: AppHandle) -> Result<usize, String> {
    let owner = owner(&app);
    tauri::async_runtime::spawn_blocking(move || recycle::restore(&ids, owner))
        .await
        .map_err(|e| e.to_string())?
}

/// Delete Recycle Bin items for good, after Windows' confirmation.
#[tauri::command]
async fn recycle_delete(ids: Vec<String>, app: AppHandle) -> Result<usize, String> {
    let owner = owner(&app);
    tauri::async_runtime::spawn_blocking(move || recycle::delete(&ids, owner))
        .await
        .map_err(|e| e.to_string())?
}

/// Shows Windows' own confirmation dialog before anything is deleted.
#[tauri::command]
async fn recycle_empty(app: AppHandle) -> Result<(), String> {
    let owner = owner(&app);
    tauri::async_runtime::spawn_blocking(move || recycle::empty(owner)).await.map_err(|e| e.to_string())?
}

/// Ctrl+N or "Open in new window": another instance of this app, optionally
/// opening a folder.
#[tauri::command]
fn new_window(path: Option<String>, transfer: Option<String>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    if let Some(p) = path {
        cmd.arg(p);
    }
    if let Some(token) = transfer {
        if token.len() != 36 || !token.chars().all(|c| c.is_ascii_hexdigit() || c == '-') { return Err("Invalid tab transfer token.".into()); }
        cmd.arg(format!("--filezy-tab={token}"));
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn window_identity(app: AppHandle) -> String { owner(&app).to_string() }

/// Convert the captured pointer's client coordinates using this window's DPI,
/// then ask Windows which top-level window is under it, even across monitors.
#[tauri::command]
fn window_at(x: f64, y: f64, scale: Option<f64>, app: AppHandle) -> Result<String, String> {
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, WindowFromPoint, GA_ROOT};
    if !x.is_finite() || !y.is_finite() { return Err("Invalid drop point.".into()); }
    let window = app.get_webview_window("main").ok_or("No window")?;
    let scale = scale.unwrap_or(window.scale_factor().map_err(|e| e.to_string())?);
    if !scale.is_finite() || !(0.5..=8.0).contains(&scale) { return Err("Invalid display scale.".into()); }
    let mut point = POINT { x: (x * scale).round() as i32, y: (y * scale).round() as i32 };
    unsafe {
        if !ClientToScreen(HWND(owner(&app) as *mut _), &mut point).as_bool() { return Err("The drop point could not be located.".into()); }
        Ok((GetAncestor(WindowFromPoint(point), GA_ROOT).0 as isize).to_string())
    }
}

#[tauri::command]
fn tab_launch_token() -> Option<String> {
    std::env::args().find_map(|arg| arg.strip_prefix("--filezy-tab=").map(str::to_string))
}

#[tauri::command]
async fn caption_place(place: caption::Place, app: AppHandle) -> Result<(), String> {
    let parent = owner(&app);
    on_main(&app, move || caption::place(parent, place.scale, place)).await?
}

/// Renderer scaling tests never change the user's Windows display settings.
#[tauri::command]
fn e2e_zoom(scale: f64, app: AppHandle) -> Result<(), String> {
    if !e2e() { return Err("not a test session".into()); }
    if !scale.is_finite() || !(0.5..=2.0).contains(&scale) { return Err("Invalid test scale.".into()); }
    app.get_webview_window("main").ok_or("No window")?.set_zoom(scale).map_err(|e| e.to_string())
}

/// Move only this disposable test window; never alter monitor settings or the
/// user's other windows. Returning native monitor DPI makes the test evidence
/// distinguish renderer zoom from an actual monitor transition.
#[tauri::command]
fn e2e_displays(index: Option<usize>, app: AppHandle) -> Result<serde_json::Value, String> {
    if !e2e() { return Err("not a test session".into()); }
    let window = app.get_webview_window("main").ok_or("No window")?;
    let monitors = window.available_monitors().map_err(|e| e.to_string())?;
    if let Some(index) = index {
        let monitor = monitors.get(index).ok_or("No such monitor")?;
        window.set_position(tauri::PhysicalPosition::new(monitor.position().x + 40, monitor.position().y + 40)).map_err(|e| e.to_string())?;
    }
    Ok(serde_json::Value::Array(monitors.iter().map(|m| serde_json::json!({
        "name": m.name(), "x": m.position().x, "y": m.position().y,
        "width": m.size().width, "height": m.size().height, "scale": m.scale_factor()
    })).collect()))
}

#[tauri::command]
async fn shell_list(path: String, devices_only: bool) -> Result<shellplaces::Folder, String> {
    let path = if devices_only { shellplaces::THIS_PC.to_string() } else { path };
    tauri::async_runtime::spawn_blocking(move || shellplaces::list(&path, devices_only)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn wsl_roots() -> Vec<(String, String)> { shellplaces::wsl_roots() }

#[tauri::command]
async fn shell_copy_out(paths: Vec<String>, app: AppHandle) -> Result<Option<OpResult>, String> {
    if paths.is_empty() || paths.len() > 10_000 { return Err("Select between 1 and 10,000 device items to copy.".into()); }
    let parent = owner(&app);
    let dest = on_main(&app, move || pick::folder(parent, "", "Copy device items to folder")).await??;
    let Some(dest) = dest else { return Ok(None) };
    Ok(Some(run_op(Op::Copy { srcs: paths, dest }, &app).await?))
}

#[derive(Serialize, Debug)]
struct LaunchTarget {
    /// Folder to open.
    path: String,
    /// Item to select in it, for Explorer's `/select,<file>`.
    select: Option<String>,
}

/// Where "Open in Filezy" goes: this user's classes, or in a test session a
/// scratch key of its own, so tests never change the user's Explorer menus.
fn classes_root() -> String {
    if e2e() { r"Software\FilezyE2E\Classes".into() } else { shellreg::CLASSES.into() }
}

fn exe_path() -> Result<String, String> {
    Ok(std::env::current_exe().map_err(|e| e.to_string())?.to_string_lossy().into_owned())
}

/// Whether Explorer's folder menus have "Open in Filezy" for this copy (shellreg.rs).
#[tauri::command]
fn explorer_entry() -> bool {
    exe_path().is_ok_and(|exe| shellreg::registered(&classes_root(), &exe))
}

#[tauri::command]
fn set_explorer_entry(on: bool) -> Result<(), String> {
    if on { shellreg::register(&classes_root(), &exe_path()?) } else { shellreg::unregister(&classes_root()) }
}

/// Whether no other Filezy window was open as this one started; only such a
/// window brings back the last tabs (session.rs).
#[tauri::command]
fn first_window() -> bool {
    session::first()
}

/// What the command line asked to show: `filezy.exe <folder>` or
/// `filezy.exe /select,<file>` (Explorer's syntax), with or without quotes.
#[tauri::command]
fn launch_target() -> Option<LaunchTarget> {
    parse_launch(std::env::args().skip(1).collect())
}

fn parse_launch(args: Vec<String>) -> Option<LaunchTarget> {
    // ExtraCursor adds the debugger switch to the app's arguments. It is not
    // part of a requested path, nor is the internal tab-handoff token.
    let joined = args.into_iter().filter(|s| !s.starts_with("--remote-debugging-port=") && !s.starts_with("--filezy-tab=") && !s.starts_with("--filezy-snap=")).collect::<Vec<_>>().join(" ");
    let raw = joined.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.get(..8).is_some_and(|prefix| prefix.eq_ignore_ascii_case("/select,")) {
        let file = raw[8..].trim().trim_matches('"').to_string();
        return Some(LaunchTarget { path: paths::parent(&file)?, select: Some(paths::file_name(&file).to_string()) });
    }
    Some(LaunchTarget { path: raw.trim_matches('"').to_string(), select: None })
}

/// True when this copy was put here by the installer rather than run in place.
/// The NSIS bundle always drops an uninstall.exe beside the app, and only an
/// installed copy can be replaced by an update.
#[tauri::command]
fn is_installed() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("uninstall.exe")))
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// A test session: a build made for the E2E runner (the `e2e` feature) that
/// the runner started with FILEZY_E2E=1. A shipped build never is one.
pub fn e2e() -> bool {
    cfg!(feature = "e2e") && std::env::var("FILEZY_E2E").is_ok_and(|v| v == "1")
}

/// A limit the E2E runner sets (FILEZY_E2E_<name>), in a test session only.
pub fn e2e_knob<T: std::str::FromStr>(name: &str) -> Option<T> {
    if !e2e() {
        return None;
    }
    std::env::var(format!("FILEZY_E2E_{name}")).ok()?.parse().ok()
}

/// Whether the page should expose its test hook (__e2e).
#[tauri::command]
fn e2e_enabled() -> bool {
    e2e()
}

/// A drag in from another app, without the real mouse (see e2edrag).
#[tauri::command]
async fn e2e_drag(paths: Vec<String>, script: e2edrag::Script, app: AppHandle) -> Result<e2edrag::Outcome, String> {
    if !e2e() {
        return Err("not a test session".into());
    }
    tauri::async_runtime::spawn_blocking(move || e2edrag::run(&app, paths, script)).await.map_err(|e| e.to_string())?
}

/// Script the next drag the page starts, then wait for it to end (see e2edrag).
#[tauri::command]
fn e2e_arm_drag(script: e2edrag::Script) -> Result<(), String> {
    if !e2e() {
        return Err("not a test session".into());
    }
    e2edrag::arm(script);
    Ok(())
}

#[tauri::command]
async fn e2e_drag_outcome() -> Result<e2edrag::Outcome, String> {
    tauri::async_runtime::spawn_blocking(e2edrag::outcome).await.map_err(|e| e.to_string())?
}

/// Open a terminal in `dir`: Windows Terminal when available, else cmd.
#[tauri::command]
fn open_terminal(dir: String) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    // A library opens at its save location; an archive has no folder to open in.
    let dir = folder(&dir)?;
    if std::process::Command::new("wt.exe").arg("-d").arg(&dir).spawn().is_ok() {
        return Ok(());
    }
    std::process::Command::new("cmd.exe")
        .args(["/K", "cd", "/d", &dir])
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Subfolder names of a folder, for the nav-pane tree and breadcrumb menus.
/// Follows the same hidden-item rules as the file list.
#[tauri::command]
async fn list_dirs(path: String, show_hidden: bool, show_protected: bool) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Folders inside an archive too: shown ("C:.zip|docs") or typed ("C:.zip\docs").
        let inside = archive::split(&path).map(|(f, i)| (f.to_string(), i.to_string())).or_else(|| {
            match paths::attributes(&path) {
                Ok(a) if a & listing::ATTR_DIRECTORY == 0 && archive::kind_of(&path).is_some() => Some((path.clone(), String::new())),
                Ok(_) => None,
                Err(_) => archive::through(&path),
            }
        });
        let entries = match inside {
            Some((file, inner)) => archive::list(&file, &inner)?,
            None => fs_list::list(&path)?,
        };
        let mut dirs: Vec<Entry> = entries
            .into_iter()
            .filter(|e| e.is_dir && listing::visible(e.attrs, show_hidden, show_protected))
            .collect();
        dirs.sort_by(|a, b| listing::cmp_name(&a.name, &b.name));
        Ok(dirs.into_iter().map(|e| e.name).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn main() {
    session::claim_first();
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::default())
        .manage(shellmenu::ShellMenu::default())
        .setup(|app| {
            clip::watch(app.handle().clone());
            drives::watch(app.handle().clone());
            // Copies of files opened from archives in earlier sessions.
            std::thread::spawn(archive::clean_staged);
            if let Some(w) = app.get_webview_window("main") {
                // The window starts hidden (tauri.conf.json) and shows where it
                // was last closed, so it never jumps there after appearing.
                let hwnd = w.hwnd().map(|h| h.0 as isize).unwrap_or(0);
                // A window split_view opened takes its half of the monitor instead.
                if hwnd == 0 || !(tools::apply_snap(hwnd) || session::restore(hwnd)) {
                    let _ = w.show();
                }
                w.on_window_event(move |e| match e {
                    // The previewer's window is owned, not a child, so it follows the window by hand.
                    tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } => {
                        preview::apply()
                    }
                    tauri::WindowEvent::CloseRequested { .. } => session::save(hwnd),
                    _ => {}
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_dir,
            cancel_listing,
            seq_base,
            get_range,
            arrange,
            resolve_path,
            search::search_start,
            search::search_stop,
            selection_stats,
            sel_keys,
            find_prefix,
            find_keys,
            sel_items,
            sel_properties,
            share,
            rename_many,
            props_for,
            props_writable,
            props_write,
            link_target,
            locale_formats,
            create_shortcuts,
            paste_shortcuts,
            launch_target,
            first_window,
            explorer_entry,
            set_explorer_entry,
            clip_copy,
            paste,
            clip_state,
            shell_menu,
            shell_menu_invoke,
            shell_menu_items,
            shell_menu_paths,
            shell_menu_native,
            copy_as_path,
            show_properties,
            open_with,
            start_drag,
            drop_paths,
            make_links,
            drop_install,
            drop_dest,
            delete_items,
            rename_item,
            create_folder,
            new_menu_items,
            create_new,
            open_path,
            archive_extract,
            archive_compress,
            pick_folder,
            network_places,
            map_drive,
            disconnect_drive,
            eject,
            preview_info,
            preview_text,
            preview_native_open,
            preview_native_place,
            preview_native_close,
            recent_files,
            recent_remove,
            recent_clear,
            get_pins,
            set_pins,
            path_thumb,
            cloud_roots,
            cloud_status,
            cloud_pin,
            open_terminal,
            new_window,
            window_identity,
            window_at,
            tab_launch_token,
            caption_place,
            shell_list,
            shell_copy_out,
            wsl_roots,
            is_installed,
            e2e_enabled,
            e2e_zoom,
            e2e_displays,
            e2e_drag,
            e2e_arm_drag,
            e2e_drag_outcome,
            recycle_paths,
            move_back,
            move_into,
            copy_into,
            path_stamps,
            recycle_list,
            recycle_restore,
            recycle_delete,
            recycle_empty,
            ext_meta,
            path_icon,
            thumb,
            drives,
            special_dirs,
            home_dir,
            list_dirs,
            tools::cancel_job,
            tools::hash_files,
            tools::compare_files,
            tools::measure_sizes,
            tools::split_view
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_arguments() {
        assert!(parse_launch(vec![]).is_none());
        assert!(parse_launch(vec!["--remote-debugging-port=9300".into(), "--filezy-tab=token".into(), "--filezy-snap=0,0,960,1040".into()]).is_none());
        let t = parse_launch(vec![r"C:\My Files\Ünï".into(), "--remote-debugging-port=9300".into()]).unwrap();
        assert_eq!(t.path, r"C:\My Files\Ünï");
        assert_eq!(parse_launch(vec!["日本語のフォルダー".into()]).unwrap().path, "日本語のフォルダー");
        let t = parse_launch(vec![r"C:\My Files\Ünï".into()]).unwrap();
        assert_eq!((t.path.as_str(), t.select), (r"C:\My Files\Ünï", None));
        // Explorer's form, as the shell splits it at the space inside the path.
        let t = parse_launch(vec![r#"/select,"C:\My"#.into(), r#"Files\report 1.txt""#.into()]).unwrap();
        assert_eq!(t.path, r"C:\My Files");
        assert_eq!(t.select.as_deref(), Some("report 1.txt"));
    }

    /// A shipped build has no test session, whatever its environment says.
    #[cfg(not(feature = "e2e"))]
    #[test]
    fn no_test_hooks_without_the_feature() {
        std::env::set_var("FILEZY_E2E", "1");
        std::env::set_var("FILEZY_E2E_SEARCH_MAX", "5");
        assert!(!e2e());
        assert_eq!(e2e_knob::<usize>("SEARCH_MAX"), None);
    }
}
