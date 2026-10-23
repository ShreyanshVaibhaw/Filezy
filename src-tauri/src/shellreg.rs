//! "Open in Filezy" in Explorer's menus for folders, drives and a folder's
//! background (F11). Opt-in from Settings and fully reversible: it lives in
//! this user's own classes (HKCU), never replaces Explorer as the file
//! manager, and turning it off, or uninstalling Filezy, deletes the keys.
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

/// Where this user's file classes live.
pub const CLASSES: &str = r"Software\Classes";
/// The verb's key under each kind of item; `%V` is the folder's path.
const PLACES: [&str; 3] = [r"Directory\shell\Filezy", r"Directory\Background\shell\Filezy", r"Drive\shell\Filezy"];

fn command(exe: &str) -> String {
    format!("\"{exe}\" \"%V\"")
}

/// Add the entry under `classes` for `exe`.
pub fn register(classes: &str, exe: &str) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for place in PLACES {
        let (verb, _) = hkcu.create_subkey(format!(r"{classes}\{place}")).map_err(|e| e.to_string())?;
        verb.set_value("", &"Open in Filezy").map_err(|e| e.to_string())?;
        verb.set_value("Icon", &format!("\"{exe}\",0")).map_err(|e| e.to_string())?;
        let (cmd, _) = verb.create_subkey("command").map_err(|e| e.to_string())?;
        cmd.set_value("", &command(exe)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Take the entry out again; nothing else under `classes` is touched.
pub fn unregister(classes: &str) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for place in PLACES {
        match hkcu.delete_subkey_all(format!(r"{classes}\{place}")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
            _ => {}
        }
    }
    Ok(())
}

/// Whether the entry is there and opens `exe` (an older copy's entry counts as off).
pub fn registered(classes: &str, exe: &str) -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    PLACES.iter().all(|place| {
        hkcu.open_subkey_with_flags(format!(r"{classes}\{place}\command"), KEY_READ)
            .and_then(|k| k.get_value::<String, _>(""))
            .is_ok_and(|v| v == command(exe))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use winreg::enums::KEY_WRITE;

    #[test]
    fn registers_and_removes_only_its_own_keys() {
        // A scratch tree of this test's own, never the real Software\Classes.
        let root = format!(r"Software\FilezyTest-{}", std::process::id());
        let classes = format!(r"{root}\Classes");
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (other, _) = hkcu.create_subkey(format!(r"{classes}\Directory\shell\SomeoneElse")).unwrap();
        other.set_value("", &"theirs").unwrap();
        let exe = r"C:\Program Files\Filezy\filezy.exe";
        assert!(!registered(&classes, exe));
        register(&classes, exe).unwrap();
        assert!(registered(&classes, exe));
        assert!(!registered(&classes, r"C:\elsewhere\filezy.exe"), "an entry for another copy isn't this one's");
        let cmd: String = hkcu.open_subkey(format!(r"{classes}\Directory\Background\shell\Filezy\command")).unwrap().get_value("").unwrap();
        assert_eq!(cmd, r#""C:\Program Files\Filezy\filezy.exe" "%V""#);
        unregister(&classes).unwrap();
        assert!(!registered(&classes, exe));
        unregister(&classes).unwrap(); // twice is fine
        let theirs: String = hkcu
            .open_subkey_with_flags(format!(r"{classes}\Directory\shell\SomeoneElse"), KEY_READ | KEY_WRITE)
            .unwrap()
            .get_value("")
            .unwrap();
        assert_eq!(theirs, "theirs", "another app's entry stayed");
        hkcu.delete_subkey_all(&root).unwrap();
    }
}
