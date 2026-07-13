use crate::fs_list::wide;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub fn shell_open(path: &str) -> Result<(), String> {
    shell_open_verb(path, "open")
}

pub fn shell_open_verb(path: &str, verb: &str) -> Result<(), String> {
    let wpath = wide(path);
    let verb = wide(verb);
    let h = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(wpath.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // Per docs, values > 32 indicate success.
    if h.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!("Could not open {path}"))
    }
}

pub fn create_folder(dir: &str) -> Result<String, String> {
    let base = Path::new(dir);
    let mut name = "New folder".to_string();
    let mut n = 2;
    while base.join(&name).exists() {
        name = format!("New folder ({n})");
        n += 1;
    }
    std::fs::create_dir(base.join(&name)).map_err(|e| e.to_string())?;
    Ok(name)
}

// Copy/move/delete now go through IFileOperation (shellmenu::file_op):
// Explorer's own engine handles conflicts, recursion guards, progress, and undo.
