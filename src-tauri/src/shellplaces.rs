//! Non-filesystem devices use shell identities throughout. Display names never
//! reach ordinary filesystem APIs. Copies use the existing shell operation log.
use serde::Serialize;
use windows::core::HSTRING;
use windows::Win32::System::SystemServices::{SFGAO_CANCOPY, SFGAO_FILESYSTEM, SFGAO_FOLDER};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, IEnumShellItems, IShellItem, SHCreateItemFromParsingName, SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY,
};

pub const THIS_PC: &str = "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}";

#[derive(Serialize)]
pub struct Item {
    pub name: String,
    pub path: String,
    pub folder: bool,
    pub copy: bool,
}
#[derive(Serialize)]
pub struct Folder {
    pub name: String,
    pub parent: Option<String>,
    pub items: Vec<Item>,
    pub capped: bool,
}

pub fn list(path: &str, devices_only: bool) -> Result<Folder, String> {
    crate::com::mta(|| list_in_mta(path, devices_only))
}

fn list_in_mta(path: &str, devices_only: bool) -> Result<Folder, String> {
    unsafe {
        let root: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path), None).map_err(|e| e.message())?;
        let name = crate::ops::item_path(&root, SIGDN_NORMALDISPLAY);
        let parent = root.GetParent().ok().map(|p| crate::ops::item_path(&p, SIGDN_DESKTOPABSOLUTEPARSING)).filter(|p| !p.is_empty());
        let enumerator: IEnumShellItems = root.BindToHandler(None, &BHID_EnumItems).map_err(|e| e.message())?;
        let mut items = Vec::new();
        let mut capped = false;
        loop {
            let mut next = [None];
            let mut count = 0;
            enumerator.Next(&mut next, Some(&mut count)).map_err(|e| e.message())?;
            if count == 0 {
                break;
            }
            let Some(item) = next[0].take() else { break };
            let attrs = item.GetAttributes(SFGAO_FOLDER | SFGAO_FILESYSTEM | SFGAO_CANCOPY).map_err(|e| e.message())?;
            if devices_only && (attrs & SFGAO_FILESYSTEM).0 != 0 {
                continue;
            }
            let path = crate::ops::item_path(&item, SIGDN_DESKTOPABSOLUTEPARSING);
            if path.is_empty() {
                continue;
            }
            if items.len() == 10_000 {
                capped = true;
                break;
            }
            items.push(Item {
                name: crate::ops::item_path(&item, SIGDN_NORMALDISPLAY),
                path,
                folder: (attrs & SFGAO_FOLDER).0 != 0,
                copy: (attrs & SFGAO_CANCOPY).0 != 0,
            });
        }
        items.sort_by(|a, b| {
            b.folder.cmp(&a.folder).then_with(|| crate::listing::sort_key(&a.name).cmp(&crate::listing::sort_key(&b.name)))
        });
        Ok(Folder { name, parent, items, capped })
    }
}

/// Discover installed distros without starting WSL or reading their files.
pub fn wsl_roots() -> Vec<(String, String)> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let Ok(root) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Lxss") else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    for key in root.enum_keys().flatten() {
        let Ok(distro) = root.open_subkey(key) else { continue };
        let Ok(name) = distro.get_value::<String, _>("DistributionName") else { continue };
        if name.is_empty() || name.contains(['\\', '/', '\0']) {
            continue;
        }
        roots.push((name.clone(), format!(r"\\wsl.localhost\{name}")));
    }
    roots.sort();
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_adapter_lists_real_identities_separately_from_names() {
        let dir = std::env::temp_dir().join(format!("filezy-shell-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Folder")).unwrap();
        std::fs::write(dir.join("日本語.txt"), "fixture").unwrap();
        let folder = list(&dir.to_string_lossy(), false).unwrap();
        assert_eq!(folder.items.len(), 2);
        assert!(folder.items[0].folder);
        assert!(folder.items.iter().all(|i| !i.path.is_empty() && i.copy));
        // Names are Explorer's: without ".txt" when it hides known extensions.
        assert!(folder.items.iter().any(|i| i.name.trim_end_matches(".txt") == "日本語"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
