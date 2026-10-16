//! Libraries (F10). A .library-ms file names several folders that Explorer
//! shows as one; Filezy lists them merged, each item under its real folder,
//! and puts new items in the library's default save location, as Explorer does.
use crate::paths;
use windows::core::HSTRING;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, STGM_READ};
use windows::Win32::UI::Shell::{
    IShellItem, IShellItemArray, IShellLibrary, SHCreateItemFromParsingName, ShellLibrary, DSFT_DETECT, LFF_FORCEFILESYSTEM,
    SIGDN_FILESYSPATH,
};

pub fn is_library(path: &str) -> bool {
    path.to_lowercase().ends_with(".library-ms")
}

fn load(file: &str) -> Result<IShellLibrary, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let lib: IShellLibrary = CoCreateInstance(&ShellLibrary, None, CLSCTX_INPROC_SERVER).map_err(|e| e.message())?;
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(file), None).map_err(|e| e.message())?;
        lib.LoadLibraryFromItem(&item, STGM_READ.0).map_err(|_| format!("'{}' isn't a library Windows can read.", paths::file_name(file)))?;
        Ok(lib)
    }
}

/// The library's folders that exist on disk, in the library's order.
pub fn folders(file: &str) -> Result<Vec<String>, String> {
    let lib = load(file)?;
    unsafe {
        let all: IShellItemArray = lib.GetFolders(LFF_FORCEFILESYSTEM).map_err(|e| e.message())?;
        let mut out = Vec::new();
        for i in 0..all.GetCount().map_err(|e| e.message())? {
            let Ok(it) = all.GetItemAt(i) else { continue };
            let p = paths::to_display(&crate::ops::item_path(&it, SIGDN_FILESYSPATH));
            if !p.is_empty() && std::path::Path::new(&paths::to_native(&p)).is_dir() {
                out.push(p);
            }
        }
        Ok(out)
    }
}

/// Where New folder, paste and drops go: the library's default save location.
pub fn save_folder(file: &str) -> Result<String, String> {
    let lib = load(file)?;
    let none = || format!("The library '{}' has no folder to save to. Add one in its properties.", paths::file_name(file));
    unsafe {
        let it: IShellItem = lib.GetDefaultSaveFolder(DSFT_DETECT).map_err(|_| none())?;
        let p = paths::to_display(&crate::ops::item_path(&it, SIGDN_FILESYSPATH));
        if p.is_empty() {
            return Err(none());
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A library file as Windows writes one, naming folders by path only.
    pub fn write(file: &std::path::Path, folders: &[&std::path::Path]) {
        let locations: String = folders
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let save = if i == 0 { "<isDefaultSaveLocation>true</isDefaultSaveLocation>" } else { "" };
                format!("<searchConnectorDescription>{save}<simpleLocation><url>{}</url></simpleLocation></searchConnectorDescription>", f.display())
            })
            .collect();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><libraryDescription xmlns="http://schemas.microsoft.com/windows/2009/library"><name>Test</name><version>1</version><isLibraryPinned>false</isLibraryPinned><searchConnectorDescriptionList>{locations}</searchConnectorDescriptionList></libraryDescription>"#
        );
        std::fs::write(file, xml).unwrap();
    }

    #[test]
    fn reads_folders_and_save_location() {
        let base = std::env::temp_dir().join(format!("filezy-lib-{}", std::process::id()));
        let (a, b) = (base.join("first"), base.join("second"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let file = base.join("Test.library-ms");
        write(&file, &[&a, &b, &base.join("gone")]);
        let file = file.to_string_lossy().to_string();
        let found = folders(&file).unwrap();
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].ends_with("first") && found[1].ends_with("second"), "{found:?}");
        assert!(save_folder(&file).unwrap().ends_with("first"));
        assert!(folders(&base.join("missing.library-ms").to_string_lossy()).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
