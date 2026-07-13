use crate::fs_list::wide;
use serde::Serialize;
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};

#[derive(Serialize)]
pub struct Drive {
    pub path: String,
    pub name: String,
    pub total: u64,
    pub free: u64,
}

pub fn list() -> Vec<Drive> {
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let wroot = wide(&root);
        let drive_type = unsafe { GetDriveTypeW(PCWSTR(wroot.as_ptr())) };
        // 3 = fixed, 2 = removable, 4 = remote
        if !(2..=4).contains(&drive_type) {
            continue;
        }
        let mut total = 0u64;
        let mut free = 0u64;
        if unsafe {
            GetDiskFreeSpaceExW(PCWSTR(wroot.as_ptr()), None, Some(&mut total), Some(&mut free))
        }
        .is_err()
        {
            continue; // not ready (empty card reader etc.)
        }
        let mut label_buf = [0u16; 261];
        let _ = unsafe {
            GetVolumeInformationW(
                PCWSTR(wroot.as_ptr()),
                Some(&mut label_buf),
                None,
                None,
                None,
                None,
            )
        };
        let len = label_buf.iter().position(|&c| c == 0).unwrap_or(0);
        let label = String::from_utf16_lossy(&label_buf[..len]);
        let label = if label.is_empty() {
            if drive_type == 4 { "Network Drive".to_string() } else { "Local Disk".to_string() }
        } else {
            label
        };
        out.push(Drive { name: format!("{label} ({letter}:)"), path: root, total, free });
    }
    out
}
