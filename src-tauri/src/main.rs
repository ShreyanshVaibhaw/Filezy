#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clip;
mod drives;
mod fs_list;
mod icons;
mod mft;
mod newmenu;
mod ops;
mod pins;
mod props;
mod recent;
mod recycle;
mod search;
mod shellmenu;
mod watch;

use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, State};

#[derive(Clone, Serialize)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: u64,
    pub created: u64,
    pub ext: String,
    pub hidden: bool,
    /// Set only on search results: the directory containing this entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

#[derive(Default)]
pub struct Listing {
    pub path: String,
    pub entries: Vec<Entry>,
    /// Indices into `entries` when a search filter is active.
    pub view: Option<Vec<u32>>,
    pub filter: String,
}

impl Listing {
    fn len(&self) -> usize {
        self.view.as_ref().map_or(self.entries.len(), |v| v.len())
    }
    fn at(&self, i: usize) -> Option<&Entry> {
        match &self.view {
            Some(v) => self.entries.get(*v.get(i)? as usize),
            None => self.entries.get(i),
        }
    }
    fn rebuild_view(&mut self) {
        if self.filter.is_empty() {
            self.view = None;
        } else {
            let q = self.filter.to_lowercase();
            self.view = Some(
                self.entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.name.to_lowercase().contains(&q))
                    .map(|(i, _)| i as u32)
                    .collect(),
            );
        }
    }
}

#[derive(Default)]
pub struct AppState {
    pub listing: Mutex<Listing>,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

#[derive(Serialize)]
pub struct OpenResult {
    path: String,
    total: usize,
}

fn entry_cmp(a: &Entry, b: &Entry, key: &str, asc: bool) -> std::cmp::Ordering {
    // Explorer always groups directories first regardless of sort direction.
    b.is_dir.cmp(&a.is_dir).then_with(|| {
        let ord = match key {
            "modified" => a.modified.cmp(&b.modified),
            "created" => a.created.cmp(&b.created),
            "size" => a.size.cmp(&b.size),
            "type" => cmp_name(&a.ext, &b.ext).then_with(|| cmp_name(&a.name, &b.name)),
            _ => cmp_name(&a.name, &b.name),
        };
        if asc { ord } else { ord.reverse() }
    })
}

fn sort_entries(entries: &mut [Entry], key: &str, asc: bool) {
    entries.sort_by(|a, b| entry_cmp(a, b, key, asc));
}

/// Sort by a document property (Author/Title/Tags/Category): read the property
/// for every entry (cached), folders first, blanks last within the direction.
fn sort_by_prop(entries: &mut [Entry], dir: &str, p: props::Prop, asc: bool) {
    use std::cmp::Ordering;
    let base = dir.trim_end_matches('\\');
    let keyed: Vec<Option<String>> = entries
        .iter()
        .map(|e| {
            if e.is_dir {
                return None;
            }
            let parent = e.parent.as_deref().unwrap_or(base);
            props::prop_of(&format!("{}\\{}", parent.trim_end_matches('\\'), e.name), p)
        })
        .collect();
    let mut idx: Vec<usize> = (0..entries.len()).collect();
    idx.sort_by(|&x, &y| {
        let (a, b) = (&entries[x], &entries[y]);
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            let ord = match (&keyed[x], &keyed[y]) {
                (Some(va), Some(vb)) => cmp_name(va, vb),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
            .then_with(|| cmp_name(&a.name, &b.name));
            if asc { ord } else { ord.reverse() }
        })
    });
    let mut sorted = Vec::with_capacity(entries.len());
    for i in &idx {
        sorted.push(entries[*i].clone());
    }
    entries.clone_from_slice(&sorted);
}

/// Property sorts stat every file; cap so a 100k system folder can't hang the sort.
const PROP_SORT_MAX: usize = 20_000;

fn apply_sort(listing: &mut Listing, key: &str, asc: bool) -> Result<(), String> {
    if let Some(p) = props::key_of(key) {
        if listing.entries.len() > PROP_SORT_MAX {
            return Err("Too many items to sort by document properties.".into());
        }
        let dir = listing.path.clone();
        sort_by_prop(&mut listing.entries, &dir, p, asc);
    } else {
        sort_entries(&mut listing.entries, key, asc);
    }
    Ok(())
}

/// Explorer-style natural sort: case-insensitive, digit runs compare numerically
/// ("file2" < "file10"). Zero-allocation so 100k-entry sorts stay instant.
// ponytail: byte-wise natural compare; StrCmpLogicalW if exact shell collation of punctuation matters
fn cmp_name(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < ab.len() && j < bb.len() {
        let (ca, cb) = (ab[i], bb[j]);
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let (si, sj) = (i, j);
            while i < ab.len() && ab[i].is_ascii_digit() {
                i += 1;
            }
            while j < bb.len() && bb[j].is_ascii_digit() {
                j += 1;
            }
            // Compare numerically: strip leading zeros, then longer run wins, then lexical.
            let (mut pa, mut pb) = (si, sj);
            while pa + 1 < i && ab[pa] == b'0' {
                pa += 1;
            }
            while pb + 1 < j && bb[pb] == b'0' {
                pb += 1;
            }
            let ord = (i - pa).cmp(&(j - pb)).then_with(|| ab[pa..i].cmp(&bb[pb..j]));
            if ord != Ordering::Equal {
                return ord;
            }
        } else {
            let (la, lb) = (ca.to_ascii_lowercase(), cb.to_ascii_lowercase());
            if la != lb {
                return la.cmp(&lb);
            }
            i += 1;
            j += 1;
        }
    }
    (ab.len() - i).cmp(&(bb.len() - j))
}

#[tauri::command]
fn open_dir(
    path: String,
    sort_key: String,
    asc: bool,
    show_hidden: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<OpenResult, String> {
    search::bump(); // cancel any in-flight search
    let norm = fs_list::normalize(&path)?;
    let mut entries = fs_list::list(&norm)?;
    if !show_hidden {
        entries.retain(|e| !e.hidden);
    }
    let total = entries.len();
    let mut listing = Listing { path: norm.clone(), entries, view: None, filter: String::new() };
    // A failing property sort (too many items) falls back to name order.
    if apply_sort(&mut listing, &sort_key, asc).is_err() {
        sort_entries(&mut listing.entries, "name", true);
    }
    *state.listing.lock().unwrap() = listing;
    watch::watch(&app, &state, &norm);
    Ok(OpenResult { path: norm, total })
}

#[tauri::command]
fn get_range(start: usize, count: usize, state: State<'_, AppState>) -> Vec<Entry> {
    let listing = state.listing.lock().unwrap();
    (start..start.saturating_add(count).min(listing.len()))
        .filter_map(|i| listing.at(i).cloned())
        .collect()
}

#[derive(Clone, Serialize)]
pub struct Group {
    pub label: String,
    pub start: usize,
    pub count: usize,
}

/// Days-since-unix-epoch -> (year, month, day). Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (rank, label) bucket for an entry under a grouping key, matching Explorer's buckets.
/// `tz_off_ms` shifts UTC timestamps to the user's local time so date buckets fall on
/// calendar-day boundaries (passed from the frontend, DST-correct for the current date).
fn group_bucket(e: &Entry, key: &str, now_ms: u64, tz_off_ms: i64) -> (u32, String) {
    match key {
        "name" => {
            let c = e.name.chars().next().unwrap_or('#').to_ascii_uppercase();
            match c {
                '0'..='9' => (0, "0 - 9".into()),
                'A'..='H' => (1, "A - H".into()),
                'I'..='P' => (2, "I - P".into()),
                'Q'..='Z' => (3, "Q - Z".into()),
                _ => (4, "Other".into()),
            }
        }
        "modified" | "created" => {
            const DAY: i64 = 86_400_000;
            let stamp = if key == "created" { e.created } else { e.modified };
            let now_day = (now_ms as i64 + tz_off_ms).div_euclid(DAY);
            let mod_day = (stamp as i64 + tz_off_ms).div_euclid(DAY).min(now_day);
            // Explorer's buckets: Today, Yesterday, Earlier this week, Last week,
            // Earlier this month, Last month, Earlier this year, A long time ago.
            let weekday = (now_day + 4).rem_euclid(7); // 0 = Sunday (1970-01-01 was a Thursday)
            let week_start = now_day - (weekday + 6) % 7; // weeks start Monday
            let (ny, nm, _) = civil_from_days(now_day);
            let (my, mm, _) = civil_from_days(mod_day);
            let last_month = if nm == 1 { (ny - 1, 12) } else { (ny, nm - 1) };
            if mod_day == now_day {
                (0, "Today".into())
            } else if mod_day == now_day - 1 {
                (1, "Yesterday".into())
            } else if mod_day >= week_start {
                (2, "Earlier this week".into())
            } else if mod_day >= week_start - 7 {
                (3, "Last week".into())
            } else if (my, mm) == (ny, nm) {
                (4, "Earlier this month".into())
            } else if (my, mm) == last_month {
                (5, "Last month".into())
            } else if my == ny {
                (6, "Earlier this year".into())
            } else {
                (7, "A long time ago".into())
            }
        }
        "size" => {
            if e.is_dir {
                return (0, "Unspecified".into());
            }
            const KB: u64 = 1024;
            const MB: u64 = KB * 1024;
            const GB: u64 = MB * 1024;
            match e.size {
                0 => (1, "Empty".into()),
                s if s <= 16 * KB => (2, "Tiny".into()),
                s if s <= MB => (3, "Small".into()),
                s if s <= 128 * MB => (4, "Medium".into()),
                s if s <= GB => (5, "Large".into()),
                s if s <= 4 * GB => (6, "Huge".into()),
                _ => (7, "Gigantic".into()),
            }
        }
        // type: rank by label alphabetically (rank unused, label compared instead)
        _ => {
            let label = icons::ext_meta(&e.ext, e.is_dir).type_name;
            if label.is_empty() {
                (0, if e.ext.is_empty() { "File".into() } else { format!("{} File", e.ext.to_uppercase()) })
            } else {
                (0, label)
            }
        }
    }
}

#[tauri::command]
fn set_group(
    key: String,
    offset_min: i64,
    sort_key: String,
    asc: bool,
    state: State<'_, AppState>,
) -> Vec<Group> {
    let mut listing = state.listing.lock().unwrap();
    if key == "none" {
        return vec![];
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let tz_off_ms = offset_min * 60_000;
    let mut keyed: Vec<(u32, String, Entry)> = listing
        .entries
        .drain(..)
        .map(|e| {
            let (rank, label) = group_bucket(&e, &key, now_ms, tz_off_ms);
            (rank, label, e)
        })
        .collect();
    // Buckets in Explorer's order; entries within each bucket follow the active sort.
    keyed.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| cmp_name(&a.1, &b.1))
            .then_with(|| entry_cmp(&a.2, &b.2, &sort_key, asc))
    });
    let mut groups: Vec<Group> = Vec::new();
    for (i, (_, label, _)) in keyed.iter().enumerate() {
        match groups.last_mut() {
            Some(g) if g.label == *label => g.count += 1,
            _ => groups.push(Group { label: label.clone(), start: i, count: 1 }),
        }
    }
    listing.entries = keyed.into_iter().map(|(_, _, e)| e).collect();
    listing.view = None;
    groups
}

// Async: property sorts read a property per file and must not run on the main thread.
#[tauri::command]
async fn set_sort(sort_key: String, asc: bool, state: State<'_, AppState>) -> Result<(), String> {
    let mut listing = state.listing.lock().unwrap();
    apply_sort(&mut listing, &sort_key, asc)?;
    listing.rebuild_view();
    Ok(())
}

#[tauri::command]
fn search_start(query: String, state: State<'_, AppState>, app: AppHandle) {
    let gen = search::bump();
    let root = {
        let mut listing = state.listing.lock().unwrap();
        listing.entries.clear();
        listing.view = None;
        listing.filter.clear();
        listing.path.clone()
    };
    std::thread::spawn(move || {
        // Whole-drive search uses the MFT index when the volume is indexable
        // (elevated); otherwise, and for subfolder scopes, walk recursively.
        if mft::can_handle(&root) && mft::search(&app, &root, &query, gen) {
            return;
        }
        search::run(app, root, query, gen);
    });
}

#[tauri::command]
fn find_index(name: String, state: State<'_, AppState>) -> Option<usize> {
    let listing = state.listing.lock().unwrap();
    (0..listing.len()).find(|&i| listing.at(i).is_some_and(|e| e.name == name))
}

#[tauri::command]
fn selection_stats(indices: Vec<usize>, state: State<'_, AppState>) -> (usize, u64) {
    let listing = state.listing.lock().unwrap();
    let size = indices
        .iter()
        .filter_map(|&i| listing.at(i))
        .filter(|e| !e.is_dir)
        .map(|e| e.size)
        .sum();
    (indices.len(), size)
}

fn selected_paths(state: &AppState, indices: &[usize]) -> (String, Vec<PathBuf>) {
    let listing = state.listing.lock().unwrap();
    let paths = indices
        .iter()
        .filter_map(|&i| {
            listing.at(i).map(|e| {
                let base = e.parent.as_deref().unwrap_or(&listing.path);
                PathBuf::from(base).join(&e.name)
            })
        })
        .collect();
    (listing.path.clone(), paths)
}

#[tauri::command]
fn clip_copy(indices: Vec<usize>, cut: bool, state: State<'_, AppState>) -> Result<(), String> {
    let (_, paths) = selected_paths(&state, &indices);
    clip::set_files(&paths, cut)
}

#[tauri::command]
async fn clip_paste(
    dest: String,
    menu: State<'_, shellmenu::ShellMenu>,
) -> Result<Vec<shellmenu::OpRecord>, String> {
    let Some((paths, cut)) = clip::get_files() else { return Ok(vec![]) };
    let srcs: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
    let menu = menu.inner().clone();
    let records = tauri::async_runtime::spawn_blocking(move || {
        menu.file_op(shellmenu::FileOp::Paste { srcs, dest, cut })
    })
    .await
    .map_err(|e| e.to_string())??;
    if cut {
        clip::clear(); // Explorer semantics: a cut can only be pasted once
    }
    Ok(records)
}

#[tauri::command]
fn clipboard_has_files() -> bool {
    clip::has_files()
}

#[tauri::command]
async fn shell_menu(
    dir: String,
    names: Vec<String>,
    menu: State<'_, shellmenu::ShellMenu>,
) -> Result<Vec<shellmenu::Item>, String> {
    menu.build(dir, names)
}

#[tauri::command]
async fn shell_menu_invoke(id: u32, menu: State<'_, shellmenu::ShellMenu>) -> Result<(), String> {
    menu.invoke(id)
}

#[tauri::command]
fn copy_as_path(indices: Vec<usize>, state: State<'_, AppState>) -> Result<(), String> {
    let (_, paths) = selected_paths(&state, &indices);
    let text = paths
        .iter()
        .map(|p| format!("\"{}\"", p.display()))
        .collect::<Vec<_>>()
        .join("\r\n");
    clip::set_text(&text)
}

#[tauri::command]
async fn show_properties(
    index: usize,
    state: State<'_, AppState>,
    menu: State<'_, shellmenu::ShellMenu>,
) -> Result<(), String> {
    let (_, paths) = selected_paths(&state, &[index]);
    let path = paths.first().ok_or("bad index")?;
    menu.properties(path.to_string_lossy().to_string())
}

#[tauri::command]
async fn start_drag(
    indices: Vec<usize>,
    state: State<'_, AppState>,
    menu: State<'_, shellmenu::ShellMenu>,
) -> Result<(), String> {
    let (_, paths) = selected_paths(&state, &indices);
    if paths.is_empty() {
        return Ok(());
    }
    let strs: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
    let menu = menu.inner().clone();
    tauri::async_runtime::spawn_blocking(move || menu.drag(strs))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn drop_paths(
    paths: Vec<String>,
    dest: String,
    menu: State<'_, shellmenu::ShellMenu>,
) -> Result<Vec<shellmenu::OpRecord>, String> {
    let dest = fs_list::normalize(&dest)?;
    let dest_path = PathBuf::from(&dest);
    // Dropping items onto their own folder is a no-op, not a self-copy.
    let srcs: Vec<String> = paths
        .into_iter()
        .filter(|p| std::path::Path::new(p).parent() != Some(dest_path.as_path()))
        .collect();
    if srcs.is_empty() {
        return Ok(vec![]);
    }
    // Explorer drop semantics: same volume moves, cross volume copies;
    // Ctrl held forces a copy, Shift held forces a move.
    let (ctrl, shift) = unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_SHIFT};
        (
            GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000 != 0,
            GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000 != 0,
        )
    };
    let same_vol = dest.len() >= 2
        && srcs.iter().all(|s| {
            s.get(..2).is_some_and(|r| r.eq_ignore_ascii_case(&dest[..2]))
        });
    let cut = if ctrl { false } else { shift || same_vol };
    let menu = menu.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        menu.file_op(shellmenu::FileOp::Paste { srcs, dest, cut })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn open_with(index: usize, state: State<'_, AppState>) -> Result<(), String> {
    let (_, paths) = selected_paths(&state, &[index]);
    let path = paths.first().ok_or("bad index")?;
    ops::shell_open_verb(&path.to_string_lossy(), "openas")
}

#[tauri::command]
async fn delete_items(
    indices: Vec<usize>,
    state: State<'_, AppState>,
    menu: State<'_, shellmenu::ShellMenu>,
) -> Result<Vec<shellmenu::OpRecord>, String> {
    let (_, paths) = selected_paths(&state, &indices);
    let paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
    let menu = menu.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        menu.file_op(shellmenu::FileOp::Delete { paths })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn rename_item(index: usize, new_name: String, state: State<'_, AppState>) -> Result<(), String> {
    let name = new_name.trim();
    // A rename is in-place: reject separators so a name can't relocate the item.
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err("A file name can't contain \\ or / characters.".into());
    }
    let (_, paths) = selected_paths(&state, &[index]);
    let from = paths.first().ok_or("bad index")?;
    // Rename within the entry's own directory (search results live outside app.path).
    let to = from.parent().ok_or("bad path")?.join(name);
    if to.exists() {
        return Err("An item with this name already exists.".into());
    }
    std::fs::rename(from, to).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_folder(dir: String) -> Result<String, String> {
    ops::create_folder(&dir)
}

#[tauri::command]
async fn new_menu_items() -> Result<Vec<newmenu::NewItem>, ()> {
    // First call scans HKCR (~100ms); cached afterwards.
    Ok(tauri::async_runtime::spawn_blocking(newmenu::items).await.unwrap_or_default())
}

#[tauri::command]
async fn create_new(dir: String, ext: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || newmenu::create(&dir, &ext))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    ops::shell_open(&path)
}

/// Properties dialog for an explicit path (drive tiles have no listing index).
#[tauri::command]
async fn show_properties_path(
    path: String,
    menu: State<'_, shellmenu::ShellMenu>,
) -> Result<(), String> {
    let menu = menu.inner().clone();
    tauri::async_runtime::spawn_blocking(move || menu.properties(path))
        .await
        .map_err(|e| e.to_string())?
}

/// Real shell icon for an arbitrary path (nav pane, Home tiles, drive tiles).
#[tauri::command]
async fn path_thumb(path: String, size: i32) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || icons::shell_icon_png(&path, size))
        .await
        .unwrap_or(None)
}

#[tauri::command]
fn get_pins() -> Vec<String> {
    pins::load()
}

#[tauri::command]
fn set_pins(pins: Vec<String>) -> Result<(), String> {
    pins::save(&pins)
}

#[tauri::command]
async fn recent_files() -> Vec<recent::RecentFile> {
    tauri::async_runtime::spawn_blocking(|| recent::list(20)).await.unwrap_or_default()
}

/// First 64 KB of a file as lossy UTF-8, for the preview pane.
#[tauri::command]
async fn read_text(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        use std::io::Read;
        let mut f = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 64 * 1024];
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        buf.truncate(n);
        Ok(String::from_utf8_lossy(&buf).into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn ext_meta(ext: String, is_dir: bool) -> icons::ExtMeta {
    icons::ext_meta(&ext, is_dir)
}

#[tauri::command]
fn path_icon(path: String) -> Option<String> {
    icons::path_icon(&path)
}

#[tauri::command]
async fn thumb(index: usize, size: i32, state: State<'_, AppState>) -> Result<Option<String>, ()> {
    let (_, paths) = selected_paths(&state, &[index]);
    let Some(path) = paths.first().map(|p| p.to_string_lossy().to_string()) else {
        return Ok(None);
    };
    Ok(tauri::async_runtime::spawn_blocking(move || icons::thumbnail(&path, size))
        .await
        .unwrap_or(None))
}

#[tauri::command]
fn drives() -> Vec<drives::Drive> {
    drives::list()
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

/// The user's OneDrive root, if any (nav-pane section like Explorer's).
#[tauri::command]
fn onedrive_dir() -> Option<String> {
    let p = std::env::var("OneDrive").ok()?;
    std::path::Path::new(&p).is_dir().then_some(p)
}

#[tauri::command]
async fn recycle_list() -> Result<Vec<recycle::RecycleItem>, String> {
    tauri::async_runtime::spawn_blocking(recycle::list).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn recycle_restore(names: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || recycle::restore(&names))
        .await
        .map_err(|e| e.to_string())?
}

/// Shows Windows' own confirmation dialog before anything is deleted.
#[tauri::command]
async fn recycle_empty() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(recycle::empty).await.map_err(|e| e.to_string())?
}

/// Undo helper: delete created copies (to the recycle bin, so it's reversible).
#[tauri::command]
async fn delete_paths(
    paths: Vec<String>,
    menu: State<'_, shellmenu::ShellMenu>,
) -> Result<(), String> {
    let menu = menu.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        menu.file_op(shellmenu::FileOp::Delete { paths })
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(())
}

/// Undo helper: put a moved/renamed item back exactly where it was.
#[tauri::command]
fn move_path(from: String, to: String) -> Result<(), String> {
    if std::path::Path::new(&to).exists() {
        return Err("Can't undo: an item with the original name already exists.".into());
    }
    std::fs::rename(&from, &to).map_err(|e| e.to_string())
}

/// Ctrl+N: another instance of this app.
#[tauri::command]
fn new_window() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe).spawn().map(|_| ()).map_err(|e| e.to_string())
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

/// Open a terminal in `dir`: Windows Terminal when available, else cmd.
#[tauri::command]
fn open_terminal(dir: String) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
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

/// Visible subdirectory names of a folder, for the nav-pane tree (lazy expansion).
#[tauri::command]
fn list_dirs(path: String) -> Vec<String> {
    fs_list::list(&path)
        .map(|entries| {
            entries
                .into_iter()
                .filter(|e| e.is_dir && !e.hidden)
                .map(|e| e.name)
                .collect()
        })
        .unwrap_or_default()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::default())
        .manage(shellmenu::ShellMenu::default())
        .invoke_handler(tauri::generate_handler![
            open_dir,
            get_range,
            set_sort,
            set_group,
            search_start,
            find_index,
            selection_stats,
            clip_copy,
            clip_paste,
            clipboard_has_files,
            shell_menu,
            shell_menu_invoke,
            copy_as_path,
            show_properties,
            open_with,
            start_drag,
            drop_paths,
            delete_items,
            rename_item,
            create_folder,
            new_menu_items,
            create_new,
            open_path,
            read_text,
            recent_files,
            show_properties_path,
            get_pins,
            set_pins,
            path_thumb,
            onedrive_dir,
            open_terminal,
            new_window,
            is_installed,
            delete_paths,
            move_path,
            recycle_list,
            recycle_restore,
            recycle_empty,
            ext_meta,
            path_icon,
            thumb,
            drives,
            special_dirs,
            home_dir,
            list_dirs
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
