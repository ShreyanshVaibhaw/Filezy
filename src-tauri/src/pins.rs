//! Quick access pins (folders) and Favorites (files), each persisted as one
//! JSON list in %APPDATA%\Filezy, in the order the user arranged them.
use std::path::PathBuf;

/// The lists there are, by name; anything else is refused.
const KINDS: [&str; 2] = ["pins", "favorites"];

fn file(kind: &str) -> Result<PathBuf, String> {
    if !KINDS.contains(&kind) {
        return Err(format!("No list called {kind}."));
    }
    let base = std::env::var("APPDATA").map_err(|_| "no APPDATA")?;
    Ok(PathBuf::from(base).join("Filezy").join(format!("{kind}.json")))
}

pub fn load(kind: &str) -> Vec<String> {
    let Ok(p) = file(kind) else { return Vec::new() };
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(kind: &str, list: &[String]) -> Result<(), String> {
    let p = file(kind)?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    std::fs::write(p, json).map_err(|e| e.to_string())
}
