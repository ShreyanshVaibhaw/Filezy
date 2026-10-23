//! Search, streaming results into the listing as they're found.
//!
//! Where the Windows index covers the scope, Windows answers the whole query
//! (index.rs): names, properties and file contents. Elsewhere the walk below
//! applies the query's names, kinds, sizes and dates itself (query.rs), and
//! the update says that file contents weren't searched.
//!
//! Link policy: junctions, symlinks and mount points show up as results but the
//! walk never descends into them. A link back to an ancestor therefore can't
//! loop and no folder is visited twice; open the link to search its target.
use crate::index::{self, Coverage};
use crate::listing::{self, Entry, Listing, ATTR_REPARSE};
use crate::query::{self, Query};
use crate::{archive, drives, fs_list, library, mft, paths, superseded, AppState, LATEST};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State};

/// Results stop here and the search says it is incomplete.
const MAX_RESULTS: usize = 100_000;

/// The cap, lowered by the E2E runner (FILEZY_E2E_SEARCH_MAX) so the capped
/// case can be tested without creating 100,000 files.
pub fn max_results() -> usize {
    static MAX: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *MAX.get_or_init(|| crate::e2e_knob("SEARCH_MAX").unwrap_or(MAX_RESULTS))
}

/// Sequence of the search the user stopped with the Stop button.
static STOPPED: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchReq {
    pub seq: u64,
    pub root: String,
    pub query: String,
    pub show_hidden: bool,
    pub show_protected: bool,
    /// "folder" (this folder only), "subfolders", "drive" (the folder's whole
    /// drive) or "all" (every drive).
    #[serde(default = "default_scope")]
    pub scope: String,
    /// Minutes to add to UTC for local time, for dates like "today".
    #[serde(default)]
    pub tz_off_min: i64,
}

fn default_scope() -> String {
    "subfolders".into()
}

#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchUpdate {
    pub seq: u64,
    pub rev: u64,
    pub total: usize,
    pub done: bool,
    /// Folders that couldn't be read (access denied, gone).
    pub skipped: usize,
    /// Results were cut off at MAX_RESULTS.
    pub capped: bool,
    /// The user pressed Stop.
    pub stopped: bool,
    /// "walk", "mft" or "index": which engine answered.
    pub provider: &'static str,
    /// What this search could and couldn't cover, said plainly.
    pub note: Option<String>,
}

#[derive(Serialize)]
pub struct SearchStarted {
    rev: u64,
    root: String,
}

/// Replace the listing with an empty result set for `root` and start filling it.
/// Async, so the index check (a COM call to the Windows Search service) never
/// runs on the UI thread.
#[tauri::command]
pub async fn search_start(req: SearchReq, state: State<'_, AppState>, app: AppHandle) -> Result<SearchStarted, String> {
    LATEST.fetch_max(req.seq, Ordering::SeqCst);
    let (root, inside, library) = match paths::resolve(&req.root)? {
        paths::Target::Dir(p) => (p, None, None),
        paths::Target::File(_) => return Err("Search needs a folder.".into()),
        paths::Target::Archive(file, inner) => (archive::location(&file, &inner), Some((file, inner)), None),
        paths::Target::Library(file) => {
            let folders = library::folders(&file)?;
            (file, None, Some(folders))
        }
    };
    if let Some((file, inner)) = inside {
        // The archive's own list answers: names, kinds, sizes and dates.
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        let q = query::parse(&req.query, now, req.tz_off_min * 60_000)?;
        let rev = install(&state, req.seq, &root)?;
        let req = SearchReq { root: root.clone(), ..req };
        std::thread::spawn(move || in_archive(app, req, &file, &inner, &q));
        return Ok(SearchStarted { rev, root });
    }
    let roots: Vec<String> = match (req.scope.as_str(), library) {
        ("all", _) => drives::list().into_iter().map(|d| d.path).collect(),
        // A library searches each of its folders.
        (_, Some(folders)) => folders,
        ("drive", None) => vec![paths::root(&root).unwrap_or_else(|| root.clone())],
        (_, None) => vec![root.clone()],
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let parsed = query::parse(&req.query, now, req.tz_off_min * 60_000);
    // Every drive is walked: the index covers only part of them.
    if roots.is_empty() {
        return Err("This library has no folders to search. Add one in its properties.".into());
    }
    // The index answers only when it covers every root (a library's folders can differ).
    let coverage = if req.scope == "all" {
        None
    } else {
        Some(roots.iter().map(|r| index::coverage(r)).find(|c| *c != Coverage::Indexed).unwrap_or(Coverage::Indexed))
    };
    let indexed = coverage == Some(Coverage::Indexed);
    // Without the index, a query the walk can't run is an error now, not an empty result.
    if !indexed {
        if let Err(e) = &parsed {
            return Err(e.clone());
        }
    }
    let rev = install(&state, req.seq, &root)?;
    let req = SearchReq { root: root.clone(), ..req };
    std::thread::spawn(move || {
        let folder_only = req.scope == "folder";
        if indexed {
            let mut sink = Sink::new(app.clone(), req.seq, "index");
            if index::search(&roots, &req.query, folder_only, req.show_hidden, req.show_protected, &mut sink) {
                if !sink.update.done {
                    sink.flush(true);
                }
                return;
            }
        }
        let note = match coverage {
            Some(Coverage::Indexed) | Some(Coverage::Unavailable) => "Windows Search isn't available, so file contents weren't searched.",
            Some(Coverage::Excluded) => "This location is excluded from the Windows index, so file contents weren't searched.",
            Some(Coverage::NotIndexed) => "This location isn't indexed, so file contents weren't searched.",
            None => "Every drive is searched by name, kind, size and date; file contents aren't searched across drives.",
        };
        let Ok(q) = parsed else {
            // The index was going to answer a query only it understands, and couldn't.
            let mut sink = Sink::new(app, req.seq, "walk");
            sink.update.note = Some("Windows Search isn't available, and this query needs it.".into());
            sink.flush(true);
            return;
        };
        // Whole-drive name searches read the MFT when the volume can be read raw (elevated).
        if roots.len() == 1 && !folder_only && !q.has_filters() && mft::can_handle(&roots[0]) && mft::search(&app, &req, &q, note) {
            return;
        }
        run(app, req, &roots, &q, folder_only, note);
    });
    Ok(SearchStarted { rev, root })
}

#[tauri::command]
pub fn search_stop(seq: u64) {
    STOPPED.fetch_max(seq, Ordering::SeqCst);
}

pub fn stopped(seq: u64) -> bool {
    STOPPED.load(Ordering::SeqCst) >= seq
}

/// Appends to the listing and reports, unless a newer request owns the listing now.
pub struct Sink {
    pub app: AppHandle,
    pub seq: u64,
    pub update: SearchUpdate,
    pub pending: Vec<Entry>,
    last: Instant,
}

impl Sink {
    pub fn new(app: AppHandle, seq: u64, provider: &'static str) -> Self {
        Sink { app, seq, update: SearchUpdate { seq, provider, ..Default::default() }, pending: Vec::new(), last: Instant::now() }
    }

    /// False once the search should end: superseded, stopped, or full.
    pub fn push(&mut self, e: Entry) -> bool {
        self.pending.push(e);
        if self.update.total + self.pending.len() >= max_results() {
            self.update.capped = true;
            return false;
        }
        if self.last.elapsed().as_millis() > 150 {
            self.last = Instant::now();
            return self.flush(false);
        }
        !self.cancelled()
    }

    pub fn cancelled(&self) -> bool {
        superseded(self.seq) || stopped(self.seq)
    }

    /// Checks the sequence under the listing lock, so a navigation that replaced
    /// the listing can never receive stale results.
    pub fn flush(&mut self, done: bool) -> bool {
        let state = self.app.state::<AppState>();
        {
            let mut l = state.listing.lock().unwrap();
            if superseded(self.seq) {
                return false;
            }
            l.entries.append(&mut self.pending);
            self.update.total = l.entries.len();
            self.update.rev = l.rev;
        }
        self.update.stopped = stopped(self.seq);
        self.update.done = done || self.update.stopped;
        let _ = self.app.emit("search-update", self.update.clone());
        !self.update.done
    }
}

/// Walk the roots (just their own entries when `folder_only`), keeping what
/// the query matches.
/// An empty listing for the results, unless a newer request owns the listing.
fn install(state: &AppState, seq: u64, root: &str) -> Result<u64, String> {
    let mut l = state.listing.lock().unwrap();
    if superseded(seq) {
        return Err("superseded".into());
    }
    *l = Listing { rev: listing::next_rev(), path: root.to_string(), ..Default::default() };
    Ok(l.rev)
}

/// Search inside an archive from its list of entries.
fn in_archive(app: AppHandle, req: SearchReq, file: &str, inner: &str, q: &Query) {
    let mut sink = Sink::new(app, req.seq, "archive");
    sink.update.note = Some("Searched by name, kind, size and date; what's inside the archive's files isn't searched.".into());
    match archive::index(file) {
        Err(e) => sink.update.note = Some(e),
        Ok(idx) => {
            let base = inner.trim_matches('\\').to_lowercase();
            let prefix = format!("{base}\\");
            for it in &idx.items {
                if sink.cancelled() {
                    break;
                }
                let lower = it.path.to_lowercase();
                let parent = it.path.rfind('\\').map_or("", |i| &it.path[..i]);
                let here = if req.scope == "folder" {
                    parent.eq_ignore_ascii_case(inner.trim_matches('\\'))
                } else {
                    base.is_empty() || lower.starts_with(&prefix)
                };
                if !here {
                    continue;
                }
                let mut e = archive::entry(it);
                if q.matches(&e) {
                    e.parent = Some(archive::location(file, parent));
                    if !sink.push(e) {
                        break;
                    }
                }
            }
        }
    }
    if !sink.update.done {
        sink.flush(true);
    }
}

pub fn run(app: AppHandle, req: SearchReq, roots: &[String], q: &Query, folder_only: bool, note: &str) {
    let mut sink = Sink::new(app, req.seq, "walk");
    sink.update.note = Some(note.into());
    let mut stack: Vec<String> = roots.iter().rev().cloned().collect();
    'walk: while let Some(dir) = stack.pop() {
        if sink.cancelled() {
            break;
        }
        let Ok(entries) = fs_list::list(&dir) else {
            sink.update.skipped += 1;
            continue;
        };
        for mut e in entries {
            if !listing::visible(e.attrs, req.show_hidden, req.show_protected) {
                continue;
            }
            if !folder_only && e.is_dir && e.attrs & ATTR_REPARSE == 0 {
                stack.push(paths::join(&dir, &e.name));
            }
            if q.matches(&e) {
                e.parent = Some(dir.clone());
                if !sink.push(e) {
                    break 'walk;
                }
            }
        }
    }
    if !sink.update.done {
        sink.flush(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_case_insensitive() {
        let entry =
            |n: &str| Entry { name: n.into(), is_dir: false, size: 0, modified: 0, created: 0, ext: String::new(), attrs: 0, parent: None };
        let q = |s: &str| query::parse(s, 0, 0).unwrap();
        assert!(q("ärger").matches(&entry("Ärger.TXT")));
        assert!(q("σοφια").matches(&entry("ΣΟΦΙΑ.md")));
        assert!(q("PORT").matches(&entry("report.pdf")));
        assert!(!q("xyz").matches(&entry("report.pdf")));
    }
}
