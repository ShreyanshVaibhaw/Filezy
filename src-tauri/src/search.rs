//! Recursive subtree search, streaming results into the listing as they're found.
//! ponytail: single-threaded walk, no MFT index yet; parallelize with rayon or add
//! a USN-journal volume index (needs elevation) if machine-wide search matters.
use crate::{fs_list, AppState, Entry};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

/// Generation counter: any navigation or new search bumps it, cancelling older walks.
pub static GEN: AtomicU64 = AtomicU64::new(0);

pub fn bump() -> u64 {
    GEN.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn generation() -> u64 {
    GEN.load(Ordering::SeqCst)
}

#[derive(Clone, Serialize)]
pub struct SearchUpdate {
    pub total: usize,
    pub done: bool,
}

pub fn run(app: AppHandle, root: String, query: String, gen: u64) {
    let ql = query.to_lowercase();
    let state = app.state::<AppState>();
    let mut stack = vec![root];
    let mut pending: Vec<Entry> = Vec::new();
    let mut last_flush = Instant::now();

    let flush = |pending: &mut Vec<Entry>, done: bool| -> bool {
        // Check the generation WHILE holding the lock so a concurrent open_dir
        // (which bumps GEN then replaces the listing under this same lock) can't
        // interleave and let us append stale results into a fresh listing.
        let total = {
            let mut listing = state.listing.lock().unwrap();
            if GEN.load(Ordering::SeqCst) != gen {
                return false;
            }
            listing.entries.append(pending);
            listing.entries.len()
        };
        let _ = app.emit("search-update", SearchUpdate { total, done });
        true
    };

    while let Some(dir) = stack.pop() {
        if GEN.load(Ordering::SeqCst) != gen {
            return;
        }
        let Ok(entries) = fs_list::list(&dir) else { continue };
        for mut e in entries {
            if e.is_dir {
                stack.push(format!("{}\\{}", dir.trim_end_matches('\\'), e.name));
            }
            if e.name.to_lowercase().contains(&ql) {
                e.parent = Some(dir.clone());
                pending.push(e);
            }
        }
        if !pending.is_empty() && last_flush.elapsed().as_millis() > 150 {
            if !flush(&mut pending, false) {
                return;
            }
            last_flush = Instant::now();
        }
    }
    flush(&mut pending, true);
}
