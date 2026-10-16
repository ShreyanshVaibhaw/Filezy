//! Shortcuts (.lnk) the way Explorer makes them: "Name - Shortcut.lnk" next to
//! the target, or in the folder a "Paste shortcut" lands in.
use crate::fs_list::wide;
use crate::paths;
use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

thread_local! {
    static COM_INIT: () = unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); };
}

/// Where a shortcut points, or None when it points at something that isn't a
/// file or folder (a Control Panel item, say).
pub fn target(path: &str) -> Option<String> {
    COM_INIT.with(|_| {});
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let w = wide(path);
        link.cast::<IPersistFile>().ok()?.Load(PCWSTR(w.as_ptr()), windows::Win32::System::Com::STGM_READ).ok()?;
        let mut buf = vec![0u16; 32768];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let n = buf.iter().position(|&c| c == 0)?;
        (n > 0).then(|| String::from_utf16_lossy(&buf[..n]))
    }
}

/// "notes.txt - Shortcut.lnk", then "(2)", "(3)"... when taken.
fn free_name(dest: &str, target: &str) -> String {
    let base = match paths::file_name(target) {
        // A drive root has no name of its own: "C - Shortcut".
        n if n.ends_with(":\\") => n.trim_end_matches(":\\").to_string(),
        n => n.to_string(),
    };
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{base} - Shortcut.lnk") } else { format!("{base} - Shortcut ({n}).lnk") };
        let p = paths::join(dest, &name);
        if paths::attributes(&p).is_err() {
            return p;
        }
        n += 1;
    }
}

/// Create one shortcut per target in `dest`; returns the shortcut paths made.
pub fn create(targets: &[String], dest: &str) -> Result<Vec<String>, String> {
    COM_INIT.with(|_| {});
    let err = |e: windows::core::Error| e.message().to_string();
    let mut made = Vec::new();
    unsafe {
        for t in targets {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(err)?;
            let wt = wide(t);
            link.SetPath(PCWSTR(wt.as_ptr())).map_err(err)?;
            if let Some(dir) = paths::parent(t) {
                let wd = wide(&dir);
                let _ = link.SetWorkingDirectory(PCWSTR(wd.as_ptr()));
            }
            let at = free_name(dest, t);
            let wa = wide(&at);
            link.cast::<IPersistFile>().map_err(err)?.Save(PCWSTR(wa.as_ptr()), true).map_err(err)?;
            made.push(at);
        }
    }
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_explorer_named_shortcuts() {
        let d = std::env::temp_dir().join(format!("filezy-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let dir = d.to_string_lossy().to_string();
        let target = paths::join(&dir, "notes.txt");
        std::fs::write(&target, "x").unwrap();
        let a = create(std::slice::from_ref(&target), &dir).unwrap();
        let b = create(&[target], &dir).unwrap();
        assert!(a[0].ends_with("notes.txt - Shortcut.lnk"), "{a:?}");
        assert!(b[0].ends_with("notes.txt - Shortcut (2).lnk"), "{b:?}");
        assert!(std::path::Path::new(&a[0]).exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
