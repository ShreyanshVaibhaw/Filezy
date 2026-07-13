//! Quick Access pins, persisted as one JSON file in %APPDATA%\Filezy.
use std::path::PathBuf;

fn file() -> Option<PathBuf> {
    let base = std::env::var("APPDATA").ok()?;
    Some(PathBuf::from(base).join("Filezy").join("pins.json"))
}

pub fn load() -> Vec<String> {
    let Some(p) = file() else { return Vec::new() };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(pins: &[String]) -> Result<(), String> {
    let p = file().ok_or("no APPDATA")?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(pins).map_err(|e| e.to_string())?;
    std::fs::write(p, json).map_err(|e| e.to_string())
}
