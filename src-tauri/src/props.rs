//! Shell properties for the Details columns, the Details pane and property
//! sorts. Values are read through each file's property handler and formatted
//! the way Explorer shows them ("00:03:25", "1920 x 1080", "red; blue"), then
//! cached by path and modified time: writing a property changes the time, so
//! an edited file is read again. Writes go through the same handler, and only
//! for properties it says are writable.
use crate::fs_list::wide;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::{
    InitPropVariantFromStringAsVector, InitPropVariantFromStringVector, PropVariantChangeType, PropVariantClear,
    PROPVARIANT, PROPVAR_CHANGE_FLAGS,
};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Variant::{VT_EMPTY, VT_LPWSTR, VT_UI4, VT_VECTOR};
use windows::Win32::UI::Shell::PropertiesSystem::{
    IPropertyDescription, IPropertyStore, IPropertyStoreCapabilities, PSCoerceToCanonicalValue,
    PSFormatForDisplayAlloc, PSGetPropertyDescription, PSGetPropertyKeyFromName, SHGetPropertyStoreFromParsingName,
    GPS_DEFAULT, GPS_READWRITE, PDFF_DEFAULT,
};

/// Filezy's names for the properties it shows, and the shell's canonical names.
const PROPS: &[(&str, &str)] = &[
    ("title", "System.Title"),
    ("author", "System.Author"),
    ("tags", "System.Keywords"),
    ("category", "System.Category"),
    ("comment", "System.Comment"),
    ("dimensions", "System.Image.Dimensions"),
    ("duration", "System.Media.Duration"),
];

/// The ones a person types (the rest are computed from the file's contents).
const EDITABLE: &[&str] = &["title", "author", "tags", "category", "comment"];

pub fn is_prop(key: &str) -> bool {
    PROPS.iter().any(|(k, _)| *k == key)
}

fn pkey(key: &str) -> Option<PROPERTYKEY> {
    static KEYS: OnceLock<HashMap<&'static str, PROPERTYKEY>> = OnceLock::new();
    // The property system needs COM on the calling thread.
    COM_INIT.with(|_| {});
    KEYS.get_or_init(|| {
        PROPS
            .iter()
            .filter_map(|(k, name)| {
                let mut pk = PROPERTYKEY::default();
                let w = wide(name);
                unsafe { PSGetPropertyKeyFromName(PCWSTR(w.as_ptr()), &mut pk) }.ok()?;
                Some((*k, pk))
            })
            .collect()
    })
    .get(key)
    .copied()
}

/// A PROPVARIANT that frees what it holds.
struct Pv(PROPVARIANT);

impl Drop for Pv {
    fn drop(&mut self) {
        unsafe { let _ = PropVariantClear(&mut self.0); }
    }
}

impl Pv {
    fn vt(&self) -> u16 {
        unsafe { self.0.Anonymous.Anonymous.vt.0 }
    }
}

thread_local! {
    static COM_INIT: () = unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); };
}

fn store(path: &str, write: bool) -> Option<IPropertyStore> {
    COM_INIT.with(|_| {});
    let w = wide(path);
    let open = || unsafe { SHGetPropertyStoreFromParsingName(PCWSTR(w.as_ptr()), None, if write { GPS_READWRITE } else { GPS_DEFAULT }) };
    // Thumbnailers, the indexer and virus scanners hold a new or changed file
    // open for a moment, and writing needs it to themselves: wait out a
    // sharing or lock violation, but nothing else (a read-only type fails at once).
    const SHARING: [u32; 4] = [0x8007_0020, 0x8007_0021, 0x8003_0020, 0x8003_0021];
    for _ in 0..15 {
        match open() {
            Ok(s) => return Some(s),
            Err(e) if write && SHARING.contains(&(e.code().0 as u32)) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(_) => return None,
        }
    }
    None
}

fn display(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<String> {
    unsafe {
        let v = Pv(store.GetValue(key).ok()?);
        if v.vt() == VT_EMPTY.0 {
            return None;
        }
        let s = PSFormatForDisplayAlloc(key, &v.0, PDFF_DEFAULT).ok()?;
        let out = s.to_string().ok();
        CoTaskMemFree(Some(s.0 as *const _));
        out.map(|o| o.trim().to_string()).filter(|o| !o.is_empty())
    }
}

/// (path, modified, key) -> value.
type Cache = HashMap<(String, u64, &'static str), Option<String>>;
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// The display values of `keys` for one file, from the cache or its handler
/// (opened once for all the keys it is missing).
pub fn values(path: &str, modified: u64, keys: &[String]) -> Vec<Option<String>> {
    let lower = path.to_lowercase();
    let known: Vec<Option<&'static str>> = keys.iter().map(|k| PROPS.iter().find(|(p, _)| p == k).map(|(p, _)| *p)).collect();
    let mut out: Vec<Option<Option<String>>> = {
        let cache = CACHE.lock().unwrap();
        known
            .iter()
            .map(|k| k.and_then(|k| cache.as_ref()?.get(&(lower.clone(), modified, k)).cloned()))
            .collect()
    };
    if out.iter().zip(&known).any(|(v, k)| v.is_none() && k.is_some()) {
        let st = store(path, false);
        let mut cache = CACHE.lock().unwrap();
        let map = cache.get_or_insert_with(HashMap::new);
        if map.len() > 200_000 {
            map.clear(); // ponytail: dumb cap; LRU if memory ever matters
        }
        for (slot, k) in out.iter_mut().zip(&known) {
            let (None, Some(k)) = (&*slot, *k) else { continue };
            let v = st.as_ref().and_then(|s| display(s, &pkey(k)?));
            map.insert((lower.clone(), modified, k), v.clone());
            *slot = Some(v);
        }
    }
    out.into_iter().map(|v| v.flatten()).collect()
}

/// One property's display value (property sorts read them one key at a time).
pub fn value(path: &str, modified: u64, key: &str) -> Option<String> {
    values(path, modified, &[key.to_string()]).pop().flatten()
}

/// An item's cloud state: Windows' STORAGE_PROVIDER_STATE value (1 online
/// only, 2 on this device, 3 always kept here, 4-6 and 10 syncing, 7 error,
/// 8 warning, 9 excluded) and its wording, as Explorer's Status shows it.
#[derive(Serialize, Clone)]
pub struct CloudState {
    pub state: u32,
    pub text: String,
}

/// Explorer's Status for items in cloud folders, as their provider reports
/// it; None for items outside cloud folders. Each folder is bound once and its
/// items read through it, the way Explorer's view reads a column: about half a
/// millisecond an item, where a property store per item takes twenty. The
/// file itself is never opened or downloaded. Never cached: syncing changes
/// the state without touching the file's time.
pub fn cloud_states(paths: &[String]) -> Vec<Option<CloudState>> {
    use windows::Win32::UI::Shell::{IShellFolder2, IShellItem, SHCreateItemFromParsingName, BHID_SFObject};
    static KEY: OnceLock<Option<PROPERTYKEY>> = OnceLock::new();
    COM_INIT.with(|_| {});
    let Some(key) = KEY
        .get_or_init(|| {
            let mut pk = PROPERTYKEY::default();
            let w = wide("System.StorageProviderState");
            unsafe { PSGetPropertyKeyFromName(PCWSTR(w.as_ptr()), &mut pk) }.ok().map(|_| pk)
        })
        .as_ref()
    else {
        return vec![None; paths.len()];
    };
    // The folder last bound, by path: callers pass a folder's items together.
    let mut bound: Option<(String, Option<IShellFolder2>)> = None;
    let mut read = |path: &str| -> Option<CloudState> {
        // The shell parses display paths, not the "\\?\" form file APIs take.
        let (dir, name) = path.trim_end_matches('\\').rsplit_once('\\')?;
        let dir = if dir.ends_with(':') { format!("{dir}\\") } else { dir.to_string() };
        if bound.as_ref().is_none_or(|(d, _)| *d != dir) {
            let w = wide(&dir);
            let item: windows::core::Result<IShellItem> = unsafe { SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None) };
            let folder = item.and_then(|item| unsafe { item.BindToHandler::<_, IShellFolder2>(None, &BHID_SFObject) }).ok();
            bound = Some((dir, folder));
        }
        let folder = bound.as_ref()?.1.as_ref()?;
        unsafe {
            let w = wide(name);
            let mut pidl = std::ptr::null_mut();
            folder.ParseDisplayName(Default::default(), None, PCWSTR(w.as_ptr()), None, &mut pidl, std::ptr::null_mut()).ok()?;
            let v = folder.GetDetailsEx(pidl, key);
            CoTaskMemFree(Some(pidl as *const _));
            // The state is a VT_UI4; anything else means the item has none.
            let v = v.ok()?;
            let state = (v.Anonymous.Anonymous.vt == VT_UI4).then(|| v.Anonymous.Anonymous.Anonymous.ulVal)?;
            Some(CloudState { state, text: state_text(key, state)? })
        }
    };
    paths.iter().map(|p| read(p)).collect()
}

/// Windows' wording for a cloud state, in the user's language.
fn state_text(key: &PROPERTYKEY, state: u32) -> Option<String> {
    let mut v = Pv(PROPVARIANT::default());
    unsafe {
        (*v.0.Anonymous.Anonymous).vt = VT_UI4;
        (*v.0.Anonymous.Anonymous).Anonymous.ulVal = state;
        let s = PSFormatForDisplayAlloc(key, &v.0, PDFF_DEFAULT).ok()?;
        let out = s.to_string().ok();
        CoTaskMemFree(Some(s.0 as *const _));
        out.filter(|o| !o.trim().is_empty())
    }
}

/// The properties a person can edit on this file: its handler opens for
/// writing and says each one is writable. A read-only file has none.
pub fn writable(path: &str) -> Vec<String> {
    let Some(st) = store(path, true) else { return vec![] };
    let caps = st.cast::<IPropertyStoreCapabilities>().ok();
    EDITABLE
        .iter()
        .filter(|k| {
            let Some(pk) = pkey(k) else { return false };
            // Without capabilities the handler doesn't say; let a write try.
            caps.as_ref().is_none_or(|c| unsafe { c.IsPropertyWritable(&pk) }.is_ok())
        })
        .map(|k| k.to_string())
        .collect()
}

#[derive(Serialize, Clone, Debug)]
pub struct Written {
    pub path: String,
    pub error: Option<String>,
}

/// Set one property on each file. Multi-valued properties (Tags, Authors,
/// Categories) take "a; b" as separate values. Each file reports its own result.
pub fn write(paths: &[String], key: &str, value: &str) -> Vec<Written> {
    paths
        .iter()
        .map(|p| Written { path: p.clone(), error: write_one(p, key, value).err() })
        .collect()
}

fn write_one(path: &str, key: &str, value: &str) -> Result<(), String> {
    if !EDITABLE.contains(&key) {
        return Err("This property can't be changed.".into());
    }
    let pk = pkey(key).ok_or("Windows doesn't know this property.")?;
    let st = store(path, true)
        .ok_or("This file's properties can't be changed: it's read-only, open in another app, or of a type without editable properties.")?;
    if let Ok(caps) = st.cast::<IPropertyStoreCapabilities>() {
        unsafe { caps.IsPropertyWritable(&pk) }.ok().map_err(|_| "This file type doesn't store this property.".to_string())?;
    }
    unsafe {
        let multi = {
            let mut desc: Option<IPropertyDescription> = None;
            PSGetPropertyDescription(&pk, &IPropertyDescription::IID, &mut desc as *mut _ as *mut _).ok();
            desc.and_then(|d| d.GetPropertyType().ok()).is_some_and(|t| t & VT_VECTOR.0 != 0)
        };
        let w = wide(value.trim());
        let mut v = if multi {
            Pv(InitPropVariantFromStringAsVector(PCWSTR(w.as_ptr())).map_err(|e| e.message().to_string())?)
        } else {
            let one = Pv(InitPropVariantFromStringVector(Some(&[PCWSTR(w.as_ptr())])).map_err(|e| e.message().to_string())?);
            let mut s = Pv(PROPVARIANT::default());
            PropVariantChangeType(&mut s.0, &one.0, PROPVAR_CHANGE_FLAGS(0), VT_LPWSTR).map_err(|e| e.message().to_string())?;
            s
        };
        if value.trim().is_empty() {
            v = Pv(PROPVARIANT::default()); // VT_EMPTY clears the property
        } else {
            PSCoerceToCanonicalValue(&pk, &mut v.0).map_err(|e| e.message().to_string())?;
        }
        st.SetValue(&pk, &v.0).map_err(|e| e.message().to_string())?;
        st.Commit().map_err(|e| e.message().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_outside_cloud_folders_have_no_cloud_state() {
        // Cloud states themselves are checked against a test provider in the E2E cloud suite.
        let d = std::env::temp_dir().join(format!("filezy-cloudstate-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("plain.txt");
        std::fs::write(&f, "x").unwrap();
        let paths = [f.to_string_lossy().to_string(), d.join("missing.txt").to_string_lossy().to_string()];
        assert!(cloud_states(&paths).iter().all(Option::is_none));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn known_keys_resolve() {
        for (k, _) in PROPS {
            assert!(pkey(k).is_some(), "{k}");
        }
        assert!(is_prop("tags") && !is_prop("size"));
    }

    #[test]
    fn a_text_file_has_no_editable_tags_and_says_so() {
        let d = std::env::temp_dir().join(format!("filezy-props-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("plain.txt");
        std::fs::write(&f, "x").unwrap();
        let p = f.to_string_lossy().to_string();
        let r = write(std::slice::from_ref(&p), "tags", "red");
        assert!(r[0].error.is_some(), "{r:?}");
        assert_eq!(values(&p, 1, &["tags".into(), "size".into()]), vec![None, None]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
