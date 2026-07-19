//! Document properties (Author/Title/Tags/Category) via the shell property
//! store, for Explorer's property-based sort fields. Reads are per-file and
//! slow, so results are cached for the process lifetime (paths are unique).
use crate::fs_list::wide;
use std::collections::HashMap;
use std::sync::Mutex;
use windows::core::{GUID, PCWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::PropertiesSystem::{
    IPropertyStore, SHGetPropertyStoreFromParsingName, GPS_DEFAULT,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Prop {
    Author,
    Title,
    Tags,
    Category,
}

// FMTID_SummaryInformation / FMTID_DocSummaryInformation, per propkey.h.
const SUMMARY: GUID = GUID::from_u128(0xF29F85E0_4FF9_1068_AB91_08002B27B3D9);
const DOC_SUMMARY: GUID = GUID::from_u128(0xD5CDD502_2E9C_101B_9397_08002B2CF9AE);

fn pkey(p: Prop) -> PROPERTYKEY {
    match p {
        Prop::Author => PROPERTYKEY { fmtid: SUMMARY, pid: 4 },
        Prop::Title => PROPERTYKEY { fmtid: SUMMARY, pid: 2 },
        Prop::Tags => PROPERTYKEY { fmtid: SUMMARY, pid: 5 },
        Prop::Category => PROPERTYKEY { fmtid: DOC_SUMMARY, pid: 2 },
    }
}

fn tag(p: Prop) -> u8 {
    match p {
        Prop::Author => 0,
        Prop::Title => 1,
        Prop::Tags => 2,
        Prop::Category => 3,
    }
}

pub fn key_of(sort_key: &str) -> Option<Prop> {
    match sort_key {
        "author" => Some(Prop::Author),
        "title" => Some(Prop::Title),
        "tags" => Some(Prop::Tags),
        "category" => Some(Prop::Category),
        _ => None,
    }
}

static CACHE: Mutex<Option<HashMap<(String, u8), Option<String>>>> = Mutex::new(None);

thread_local! {
    static COM_INIT: () = unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); };
}

fn read(path: &str, p: Prop) -> Option<String> {
    COM_INIT.with(|_| {});
    unsafe {
        let w = wide(path);
        let store: IPropertyStore =
            SHGetPropertyStoreFromParsingName(PCWSTR(w.as_ptr()), None, GPS_DEFAULT).ok()?;
        let v = store.GetValue(&pkey(p)).ok()?;
        let s = PropVariantToStringAlloc(&v).ok()?;
        let out = s.to_string().ok()?;
        windows::Win32::System::Com::CoTaskMemFree(Some(s.0 as *const _));
        (!out.trim().is_empty()).then(|| out.trim().to_string())
    }
}

pub fn prop_of(path: &str, p: Prop) -> Option<String> {
    let key = (path.to_lowercase(), tag(p));
    {
        let cache = CACHE.lock().unwrap();
        if let Some(map) = cache.as_ref() {
            if let Some(v) = map.get(&key) {
                return v.clone();
            }
        }
    }
    let v = read(path, p);
    let mut cache = CACHE.lock().unwrap();
    let map = cache.get_or_insert_with(HashMap::new);
    if map.len() > 100_000 {
        map.clear(); // ponytail: dumb cap; LRU if memory ever matters
    }
    map.insert(key, v.clone());
    v
}
