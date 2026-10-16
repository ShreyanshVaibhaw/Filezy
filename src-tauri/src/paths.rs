//! Windows path forms.
//!
//! The UI and every command use display paths: "C:\Users\me", "\\server\share\dir".
//! Win32 file APIs get the native extended-length form ("\\?\C:\Users\me",
//! "\\?\UNC\server\share\dir") so names past MAX_PATH work. Shell parsing names
//! ("::{GUID}", "shell:Downloads") are neither and never reach a file API.
use crate::fs_list::wide;
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    FindClose, FindFirstFileW, GetFileAttributesW, GetFullPathNameW, GetVolumeNameForVolumeMountPointW,
    GetVolumePathNameW, FILE_ATTRIBUTE_DIRECTORY, INVALID_FILE_ATTRIBUTES, WIN32_FIND_DATAW,
};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Kind {
    /// "C:\..." or a bare "C:"
    Drive,
    /// "\\server\share\..." or a bare "\\server"
    Unc,
    /// "::{GUID}...", "shell:..." and other shell namespace names
    Shell,
    /// Relative or malformed
    Other,
}

pub fn kind(p: &str) -> Kind {
    let b = p.as_bytes();
    if p.starts_with("::") || p.get(..6).is_some_and(|s| s.eq_ignore_ascii_case("shell:")) {
        Kind::Shell
    } else if p.starts_with(r"\\?\") {
        let d = to_display(p);
        if d.starts_with(r"\\?\") { Kind::Other } else { kind(&d) }
    } else if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b.len() == 2 || b[2] == b'\\') {
        Kind::Drive
    } else if unc_parts(p).is_some() {
        Kind::Unc
    } else {
        Kind::Other
    }
}

/// ("server", "share", "rest") for "\\server\share\rest"; share and rest may be empty.
fn unc_parts(p: &str) -> Option<(&str, &str, &str)> {
    let body = p.strip_prefix(r"\\")?;
    if body.starts_with(['\\', '?', '.']) {
        return None;
    }
    let mut it = body.splitn(3, '\\');
    let server = it.next().filter(|s| !s.is_empty())?;
    Some((server, it.next().unwrap_or(""), it.next().unwrap_or("")))
}

/// "\\?\C:\x" -> "C:\x", "\\?\UNC\srv\sh\x" -> "\\srv\sh\x". Other forms are returned as is,
/// including volume GUID paths that have no drive-letter equivalent.
pub fn to_display(p: &str) -> String {
    if let Some(rest) = p.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = p.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            return rest.to_string();
        }
    }
    p.to_string()
}

/// Extended-length form for Win32 file APIs. The input must already be a
/// normalized display path: "\\?\" turns off the API's own "." and ".." handling.
pub fn to_native(p: &str) -> String {
    match kind(p) {
        Kind::Drive if p.starts_with(r"\\?\") => p.to_string(),
        Kind::Drive if p.len() == 2 => format!(r"\\?\{p}\"),
        Kind::Drive => format!(r"\\?\{p}"),
        Kind::Unc if p.starts_with(r"\\?\") => p.to_string(),
        Kind::Unc => format!(r"\\?\UNC\{}", &p[2..]),
        _ => p.to_string(),
    }
}

/// Root of a display path: "C:\" or "\\server\share\". None for a bare server or shell name.
pub fn root(p: &str) -> Option<String> {
    match kind(p) {
        Kind::Drive => Some(format!("{}\\", &p[..2])),
        Kind::Unc => match unc_parts(p)? {
            (_, "", _) => None,
            (server, share, _) => Some(format!(r"\\{server}\{share}\")),
        },
        _ => None,
    }
}

/// The containing folder, or None at a drive root, a bare server, or a shell name.
/// The parent of a share root is its server ("\\server").
pub fn parent(p: &str) -> Option<String> {
    match kind(p) {
        Kind::Drive => {
            let t = p.trim_end_matches('\\');
            let i = t.rfind('\\')?;
            Some(if i <= 2 { format!("{}\\", &t[..2]) } else { t[..i].to_string() })
        }
        Kind::Unc => {
            let (server, share, rest) = unc_parts(p)?;
            let rest = rest.trim_end_matches('\\');
            if share.is_empty() {
                None
            } else if rest.is_empty() {
                Some(format!(r"\\{server}"))
            } else {
                Some(match rest.rfind('\\') {
                    Some(i) => format!(r"\\{server}\{share}\{}", &rest[..i]),
                    None => format!(r"\\{server}\{share}"),
                })
            }
        }
        _ => None,
    }
}

pub fn join(dir: &str, name: &str) -> String {
    format!("{}\\{}", dir.trim_end_matches('\\'), name)
}

/// Last component as Explorer shows it: "Users" for "C:\Users", "C:\" for a root.
pub fn file_name(p: &str) -> &str {
    let t = p.trim_end_matches('\\');
    match t.rfind('\\') {
        Some(i) if i + 1 < t.len() => &t[i + 1..],
        _ => p,
    }
}

/// Why Windows would refuse `name` for a file or folder, in Explorer's words,
/// after Explorer's own trimming. Ok carries the name to actually use.
pub fn check_name(name: &str) -> Result<String, String> {
    // Win32 drops trailing dots and spaces; through "\\?\" paths they would
    // survive and make an item nothing else can open, so trim them here.
    let n = name.trim_start_matches(' ').trim_end_matches([' ', '.']);
    if n.is_empty() {
        return Err("Type a name.".into());
    }
    if n.chars().any(|c| matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || (c as u32) < 32) {
        return Err(r#"A file name can't contain any of the following characters: \ / : * ? " < > |"#.into());
    }
    let stem = n.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0');
    if reserved {
        return Err("The specified device name is invalid.".into());
    }
    if n.encode_utf16().count() > 255 {
        return Err("The file name is too long.".into());
    }
    Ok(n.to_string())
}

fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn full_path(p: &str) -> Result<String, String> {
    let w = wide(p);
    unsafe {
        let need = GetFullPathNameW(PCWSTR(w.as_ptr()), None, None);
        if need == 0 {
            return Err(windows::core::Error::from_thread().message().to_string());
        }
        let mut buf = vec![0u16; need as usize + 1];
        let got = GetFullPathNameW(PCWSTR(w.as_ptr()), Some(&mut buf), None);
        if got == 0 {
            return Err(windows::core::Error::from_thread().message().to_string());
        }
        Ok(from_wide(&buf))
    }
}

/// Raw attributes of a display path, or the Win32 error code.
pub fn attributes(p: &str) -> Result<u32, u32> {
    let w = wide(&to_native(p));
    let a = unsafe { GetFileAttributesW(PCWSTR(w.as_ptr())) };
    if a == INVALID_FILE_ATTRIBUTES {
        Err(unsafe { windows::Win32::Foundation::GetLastError() }.0)
    } else {
        Ok(a)
    }
}

/// Explorer's wording for why a location can't be opened.
pub fn open_error(input: &str, code: u32) -> String {
    let what = input.trim();
    match code {
        5 => format!("You don't have permission to open '{what}'."),
        21 => format!("'{what}' is not ready. Insert a disc or reconnect the drive."),
        2 | 3 | 53 | 67 | 123 | 161 => not_found(input),
        _ => {
            let hr = windows::core::HRESULT::from_win32(code);
            format!("{what}: {}", windows::core::Error::from(hr).message())
        }
    }
}

/// Each component in its on-disk case, with 8.3 names expanded, without
/// following links: entering a junction keeps the junction's own path.
fn true_case(p: &str) -> String {
    let Some(root) = root(p) else { return p.to_string() };
    let root = if kind(p) == Kind::Drive { root.to_ascii_uppercase() } else { root };
    let mut out = root.trim_end_matches('\\').to_string();
    let rest = p.get(root.len()..).unwrap_or("");
    for comp in rest.split('\\').filter(|c| !c.is_empty()) {
        let probe = format!("{out}\\{comp}");
        let w = wide(&to_native(&probe));
        let mut data = WIN32_FIND_DATAW::default();
        let real = unsafe { FindFirstFileW(PCWSTR(w.as_ptr()), &mut data) }
            .ok()
            .map(|h| {
                unsafe { let _ = FindClose(h); }
                from_wide(&data.cFileName)
            })
            .filter(|n| n.eq_ignore_ascii_case(comp) || comp.contains('~'));
        out = format!("{out}\\{}", real.as_deref().unwrap_or(comp));
    }
    if out.len() == 2 { format!("{out}\\") } else { out }
}

fn expand_env(s: &str) -> String {
    use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
    let w = wide(s);
    unsafe {
        let need = ExpandEnvironmentStringsW(PCWSTR(w.as_ptr()), None);
        if need == 0 {
            return s.to_string();
        }
        let mut buf = vec![0u16; need as usize];
        ExpandEnvironmentStringsW(PCWSTR(w.as_ptr()), Some(&mut buf));
        from_wide(&buf)
    }
}

/// Filesystem folder behind a shell name such as "shell:Downloads", if it has one.
fn shell_folder(s: &str) -> Option<String> {
    use windows::Win32::UI::Shell::{SHGetNameFromIDList, SHParseDisplayName, SIGDN_FILESYSPATH};
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    let w = wide(s);
    unsafe {
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut pidl, 0, None).ok()?;
        let name = SHGetNameFromIDList(pidl, SIGDN_FILESYSPATH);
        windows::Win32::System::Com::CoTaskMemFree(Some(pidl as *const _));
        let name = name.ok()?;
        let out = name.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(name.0 as *const _));
        out
    }
}

/// What an address-bar entry points at.
#[derive(Debug, PartialEq)]
pub enum Target {
    Dir(String),
    File(String),
    /// A folder inside an archive: the archive's path, and the folder in it
    /// ("" for its root) in the archive's own case.
    Archive(String, String),
    /// A library (.library-ms): several folders shown as one.
    Library(String),
}

pub fn not_found(input: &str) -> String {
    format!("Windows can't find '{}'. Check the spelling and try again.", input.trim())
}

/// Resolve user input (typed, pasted with quotes, "%USERPROFILE%\x", "shell:Downloads",
/// "C:\a\..\b") to an existing item's display path in its on-disk case.
pub fn resolve(input: &str) -> Result<Target, String> {
    // An archive location ("C:\a.zip|docs"): the archive, then the folder in it.
    if let Some((file, inner)) = crate::archive::split(input.trim().trim_matches('"')) {
        return match resolve(file)? {
            Target::File(f) if crate::archive::kind_of(&f).is_some() => match crate::archive::folder(&f, inner)? {
                Some(real) => Ok(Target::Archive(f, real)),
                None => Err(format!("'{}' isn't a folder in '{}'.", inner, file_name(&f))),
            },
            _ => Err(not_found(input)),
        };
    }
    let mut s = input.trim().trim_matches('"').replace('/', "\\");
    if s.contains('%') {
        s = expand_env(&s);
    }
    if kind(&s) == Kind::Shell && !s.starts_with("::") {
        s = shell_folder(&s).ok_or_else(|| not_found(input))?;
    }
    if s.len() == 2 && s.ends_with(':') {
        s.push('\\');
    }
    if !matches!(kind(&s), Kind::Drive | Kind::Unc) {
        return Err(not_found(input));
    }
    let disp = to_display(&full_path(&s).map_err(|_| not_found(input))?);
    // A bare "\\server" is a location of its own (its shares), not a folder.
    if kind(&disp) == Kind::Unc && root(&disp).is_none() {
        return Ok(Target::Dir(disp.trim_end_matches('\\').to_string()));
    }
    let attrs = match attributes(&disp) {
        Ok(a) => a,
        Err(code) => {
            // A typed path that runs through an archive: "C:\a.zip\docs".
            if let Some((file, inner)) = crate::archive::through(&disp) {
                if let Ok(Some(real)) = crate::archive::folder(&file, &inner) {
                    return Ok(Target::Archive(true_case(&file), real));
                }
            }
            return Err(open_error(input, code));
        }
    };
    let real = true_case(&disp);
    Ok(if attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
        Target::Dir(real)
    } else if crate::library::is_library(&real) {
        Target::Library(real)
    } else {
        Target::File(real)
    })
}

/// Identity of the volume holding `p`, for Explorer's drop rule (same volume
/// moves, different volume copies). Volume GUID paths tell mount points and
/// SUBST drives apart where drive letters can't; shares compare by their root.
pub fn volume_id(p: &str) -> Option<String> {
    let w = wide(&to_native(p));
    let mut mount = [0u16; 1024];
    unsafe { GetVolumePathNameW(PCWSTR(w.as_ptr()), &mut mount) }.ok()?;
    let mount = from_wide(&mount);
    let mut guid = [0u16; 64];
    let mw = wide(&to_display(&mount));
    if unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(mw.as_ptr()), &mut guid) }.is_ok() {
        Some(from_wide(&guid).to_ascii_lowercase())
    } else {
        Some(to_display(&mount).trim_end_matches('\\').to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(kind(r"C:\x"), Kind::Drive);
        assert_eq!(kind("c:"), Kind::Drive);
        assert_eq!(kind(r"\\srv\share\x"), Kind::Unc);
        assert_eq!(kind(r"\\srv"), Kind::Unc);
        assert_eq!(kind(r"\\?\C:\x"), Kind::Drive);
        assert_eq!(kind(r"\\?\UNC\srv\share"), Kind::Unc);
        assert_eq!(kind("::{645FF040-5081-101B-9F08-00AA002F954E}"), Kind::Shell);
        assert_eq!(kind("Shell:Downloads"), Kind::Shell);
        assert_eq!(kind(r"rel\x"), Kind::Other);
        assert_eq!(kind(r"\\.\C:"), Kind::Other);
        assert_eq!(kind("C:x"), Kind::Other);
    }

    #[test]
    fn display_and_native_round_trip() {
        assert_eq!(to_display(r"\\?\C:\a\b"), r"C:\a\b");
        assert_eq!(to_display(r"\\?\UNC\srv\sh\d"), r"\\srv\sh\d");
        assert_eq!(to_display(r"\\?\Volume{1234}\x"), r"\\?\Volume{1234}\x");
        assert_eq!(to_native(r"C:\a"), r"\\?\C:\a");
        assert_eq!(to_native("C:"), r"\\?\C:\");
        assert_eq!(to_native(r"\\srv\sh\d"), r"\\?\UNC\srv\sh\d");
        assert_eq!(to_native(r"\\?\C:\a"), r"\\?\C:\a");
        for p in [r"C:\a\b c\ü", r"\\srv\sh\x y"] {
            assert_eq!(to_display(&to_native(p)), p);
        }
    }

    #[test]
    fn roots_and_parents() {
        assert_eq!(root(r"C:\a\b").as_deref(), Some(r"C:\"));
        assert_eq!(root(r"\\srv\sh\a").as_deref(), Some(r"\\srv\sh\"));
        assert_eq!(root(r"\\srv"), None);
        assert_eq!(parent(r"C:\a\b").as_deref(), Some(r"C:\a"));
        assert_eq!(parent(r"C:\a").as_deref(), Some(r"C:\"));
        assert_eq!(parent(r"C:\a\").as_deref(), Some(r"C:\"));
        assert_eq!(parent(r"C:\"), None);
        assert_eq!(parent(r"\\srv\sh\a\b").as_deref(), Some(r"\\srv\sh\a"));
        assert_eq!(parent(r"\\srv\sh\a").as_deref(), Some(r"\\srv\sh"));
        assert_eq!(parent(r"\\srv\sh").as_deref(), Some(r"\\srv"));
        assert_eq!(parent(r"\\srv"), None);
        assert_eq!(parent("::{X}"), None);
    }

    #[test]
    fn names() {
        assert_eq!(join(r"C:\", "a"), r"C:\a");
        assert_eq!(join(r"C:\a", "b"), r"C:\a\b");
        assert_eq!(join(r"\\srv\sh", "b"), r"\\srv\sh\b");
        assert_eq!(file_name(r"C:\a\b"), "b");
        assert_eq!(file_name(r"C:\"), r"C:\");
        assert_eq!(file_name(r"\\srv\sh"), "sh");
    }

    #[test]
    fn name_rules() {
        assert_eq!(check_name("report.txt"), Ok("report.txt".into()));
        assert_eq!(check_name("  trailing. . "), Ok("trailing".into()));
        assert!(check_name("a/b").is_err());
        assert!(check_name("a:b").is_err());
        assert!(check_name("tab\tname").is_err());
        assert!(check_name("...").is_err());
        assert!(check_name("   ").is_err());
        assert!(check_name("CON").is_err());
        assert!(check_name("con.txt").is_err());
        assert!(check_name("LPT1.log").is_err());
        assert_eq!(check_name("COM0"), Ok("COM0".into()));
        assert_eq!(check_name("CONSOLE"), Ok("CONSOLE".into()));
        assert!(check_name(&"x".repeat(256)).is_err());
        assert!(check_name(&"x".repeat(255)).is_ok());
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("filezy-paths-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn resolve_fixes_case_and_keeps_links() {
        let d = temp_dir("case");
        let real = d.join("RealFolder");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("File.TXT"), "x").unwrap();
        let base = to_display(&full_path(d.to_str().unwrap()).unwrap());
        let typed = format!(r"{}\realfolder\..\REALFOLDER", base.to_lowercase());
        assert_eq!(resolve(&typed), Ok(Target::Dir(true_case(&format!(r"{base}\RealFolder")))));
        assert!(matches!(resolve(&format!(r#""{base}\RealFolder\file.txt""#)), Ok(Target::File(f)) if f.ends_with(r"RealFolder\File.TXT")));
        // A junction is entered by its own name, not its target's.
        let link = d.join("Link");
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J", link.to_str().unwrap(), real.to_str().unwrap()])
            .output()
            .is_ok_and(|o| o.status.success());
        if ok {
            match resolve(link.to_str().unwrap()) {
                Ok(Target::Dir(p)) => assert!(p.ends_with(r"\Link"), "{p}"),
                other => panic!("{other:?}"),
            }
        }
        assert!(resolve(&format!(r"{base}\missing")).unwrap_err().starts_with("Windows can't find"));
        assert!(resolve("relative").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn resolve_long_paths() {
        let d = temp_dir("long");
        let mut p = to_display(&full_path(d.to_str().unwrap()).unwrap());
        while p.len() < 300 {
            p = join(&p, "abcdefghijklmnopqrstuvwxyz0123456789");
        }
        std::fs::create_dir_all(to_native(&p)).unwrap();
        assert_eq!(resolve(&p), Ok(Target::Dir(true_case(&p))));
        assert!(attributes(&p).is_ok());
        let _ = std::fs::remove_dir_all(to_native(&to_display(&full_path(d.to_str().unwrap()).unwrap())));
    }

    #[test]
    fn same_volume() {
        let tmp = std::env::temp_dir();
        let a = volume_id(tmp.to_str().unwrap()).unwrap();
        let b = volume_id(&format!("{}\\x", tmp.to_str().unwrap())).unwrap();
        assert_eq!(a, b);
        assert!(a.starts_with(r"\\?\volume{"), "{a}");
    }
}
