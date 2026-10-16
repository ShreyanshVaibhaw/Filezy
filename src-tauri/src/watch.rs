use crate::{paths, AppState};
use notify::{RecursiveMode, Watcher};
use std::path::Path;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Watch `path` (a display path), replacing any previous watcher. Changes are
/// debounced and emitted as one "fs-changed" event carrying the path. Watcher
/// errors (buffer overflow, the folder itself vanishing) also emit, so the view
/// refreshes and finds out instead of silently going stale.
/// Returns false when Windows won't report changes for this location.
pub fn watch(app: &AppHandle, state: &AppState, path: &str) -> bool {
    let mut guard = state.watcher.lock().unwrap();
    *guard = None; // drop previous watcher; its channel closes, its thread exits

    // Inside an archive, the archive file is what changes: watch its folder
    // for changes to that one file. A library changes in any of its folders.
    let (dirs, only) = match crate::archive::split(path) {
        Some((file, _)) => match paths::parent(file) {
            Some(dir) => (vec![dir], Some(paths::to_native(file).to_lowercase())),
            None => return false,
        },
        None if crate::library::is_library(path) => match crate::library::folders(path) {
            Ok(folders) if !folders.is_empty() => (folders, None),
            _ => return false,
        },
        None => (vec![path.to_string()], None),
    };
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let Ok(mut watcher) = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let ours = match (&only, &res) {
            (Some(file), Ok(ev)) => ev.paths.iter().any(|p| p.to_string_lossy().to_lowercase() == *file),
            _ => true,
        };
        if ours {
            let _ = tx.send(());
        }
    }) else {
        return false;
    };
    for dir in &dirs {
        if watcher.watch(Path::new(&paths::to_native(dir)), RecursiveMode::NonRecursive).is_err() {
            return false;
        }
    }

    let app = app.clone();
    let watched = path.to_string();
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            // Debounce: swallow the burst, then notify once.
            let deadline = Instant::now() + Duration::from_millis(80);
            loop {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                if rx.recv_timeout(deadline - now).is_err() {
                    break;
                }
            }
            let _ = app.emit("fs-changed", &watched);
        }
    });

    *guard = Some(watcher);
    true
}
