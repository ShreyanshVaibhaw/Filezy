//! Explorer's "New >" submenu, rebuilt from the same source: HKCR ShellNew
//! registrations. Each extension registers one of four creation methods:
//! NullFile (empty file), Data (inline bytes), FileName (copy a template),
//! or Command (run a wizard, e.g. the Shortcut wizard).
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use winreg::enums::HKEY_CLASSES_ROOT;
use winreg::RegKey;

#[derive(Clone)]
enum Method {
    NullFile,
    Data(Vec<u8>),
    FileName(String),
    Command(String),
}

#[derive(Clone)]
struct Template {
    ext: String,
    name: String,
    method: Method,
}

#[derive(Clone, Serialize)]
pub struct NewItem {
    pub ext: String,
    pub name: String,
}

static CACHE: OnceLock<Vec<Template>> = OnceLock::new();

fn templates() -> &'static [Template] {
    CACHE.get_or_init(scan)
}

fn shellnew_method(key: &RegKey) -> Option<Method> {
    if let Ok(cmd) = key.get_value::<String, _>("Command") {
        return Some(Method::Command(cmd));
    }
    if let Ok(f) = key.get_value::<String, _>("FileName") {
        return Some(Method::FileName(f));
    }
    if let Ok(v) = key.get_raw_value("Data") {
        return Some(Method::Data(v.bytes));
    }
    if key.get_raw_value("NullFile").is_ok() {
        return Some(Method::NullFile);
    }
    None // Handler-only (COM) entries are skipped
}

fn scan() -> Vec<Template> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let mut out: Vec<Template> = Vec::new();
    for ext_name in hkcr.enum_keys().filter_map(Result::ok) {
        if !ext_name.starts_with('.') {
            continue;
        }
        let Ok(ext_key) = hkcr.open_subkey(&ext_name) else { continue };
        // Like Explorer: ShellNew directly under the extension, then under the
        // default progid, then under ANY progid subkey (e.g. .txt keeps its
        // ShellNew under txtfile even when another app owns the extension).
        let method = ext_key
            .open_subkey("ShellNew")
            .ok()
            .and_then(|k| shellnew_method(&k))
            .or_else(|| {
                let progid: String = ext_key.get_value("").ok()?;
                if progid.is_empty() {
                    return None;
                }
                let k = ext_key.open_subkey(format!("{progid}\\ShellNew")).ok()?;
                shellnew_method(&k)
            })
            .or_else(|| {
                ext_key.enum_keys().filter_map(Result::ok).find_map(|sub| {
                    let k = ext_key.open_subkey(format!("{sub}\\ShellNew")).ok()?;
                    shellnew_method(&k)
                })
            });
        let Some(method) = method else { continue };
        let ext = ext_name.trim_start_matches('.').to_lowercase();
        if ext.is_empty() {
            continue;
        }
        let name = crate::icons::ext_meta(&ext, false).type_name;
        if name.is_empty() {
            continue;
        }
        out.push(Template { ext, name, method });
    }
    // Win11's Store Notepad registers .txt ShellNew via AppX manifest plumbing that
    // may never appear in HKCR; Explorer still shows Text Document, so we do too.
    if !out.iter().any(|t| t.ext == "txt") {
        let name = crate::icons::ext_meta("txt", false).type_name;
        out.push(Template {
            ext: "txt".into(),
            name: if name.is_empty() { "Text Document".into() } else { name },
            method: Method::NullFile,
        });
    }
    // Explorer pins Shortcut right after Folder; the rest reads best alphabetically.
    out.sort_by(|a, b| {
        (a.ext != "lnk")
            .cmp(&(b.ext != "lnk"))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out.dedup_by(|a, b| a.ext == b.ext);
    out
}

pub fn items() -> Vec<NewItem> {
    templates().iter().map(|t| NewItem { ext: t.ext.clone(), name: t.name.clone() }).collect()
}

fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        match rest[i + 1..].find('%') {
            Some(j) => {
                let var = &rest[i + 1..i + 1 + j];
                match std::env::var(var) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(var);
                        out.push('%');
                    }
                }
                rest = &rest[i + j + 2..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Resolve a ShellNew FileName value: absolute path, or a name looked up in the
/// user's Templates folder then %SystemRoot%\ShellNew (Explorer's search order).
fn find_template(file: &str) -> Option<PathBuf> {
    let f = expand_env(file);
    let p = Path::new(&f);
    if p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        bases.push(PathBuf::from(appdata).join(r"Microsoft\Windows\Templates"));
    }
    if let Ok(windir) = std::env::var("SystemRoot") {
        bases.push(PathBuf::from(windir).join("ShellNew"));
    }
    bases.into_iter().map(|b| b.join(&f)).find(|c| c.exists())
}

/// Create a new item of the registered type in `dir`. Returns the created file
/// name, or None when the type is command-driven (a wizard was launched instead).
pub fn create(dir: &str, ext: &str) -> Result<Option<String>, String> {
    let t = templates().iter().find(|t| t.ext == ext).ok_or("Unknown file type")?.clone();

    if let Method::Command(cmd) = &t.method {
        let quoted = format!("\"{dir}\"");
        let cmd = expand_env(cmd).replace("\"%1\"", "%1").replace("%1", &quoted);
        std::process::Command::new("cmd")
            .args(["/C", &cmd])
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(None);
    }

    let base = Path::new(dir);
    let mut name = format!("New {}.{}", t.name, t.ext);
    let mut n = 2;
    while base.join(&name).exists() {
        name = format!("New {} ({n}).{}", t.name, t.ext);
        n += 1;
    }
    let dest = base.join(&name);
    match &t.method {
        Method::NullFile => {
            std::fs::File::create(&dest).map_err(|e| e.to_string())?;
        }
        Method::Data(bytes) => {
            std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
        }
        Method::FileName(f) => {
            let tpl = find_template(f).ok_or_else(|| format!("Template not found: {f}"))?;
            std::fs::copy(&tpl, &dest).map_err(|e| e.to_string())?;
        }
        Method::Command(_) => unreachable!(),
    }
    Ok(Some(name))
}
