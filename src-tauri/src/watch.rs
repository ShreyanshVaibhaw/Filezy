use crate::AppState;
use notify::{RecursiveMode, Watcher};
use std::path::Path;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Watch `path`, replacing any previous watcher. Changes are debounced and
/// emitted as a single "fs-changed" event carrying the watched path.
pub fn watch(app: &AppHandle, state: &AppState, path: &str) {
    let mut guard = state.watcher.lock().unwrap();
    *guard = None; // drop previous watcher; its channel closes, its thread exits

    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let Ok(mut watcher) = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok() {
            let _ = tx.send(());
        }
    }) else {
        return;
    };
    if watcher.watch(Path::new(path), RecursiveMode::NonRecursive).is_err() {
        return;
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
}
