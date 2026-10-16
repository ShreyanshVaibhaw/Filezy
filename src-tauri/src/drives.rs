//! Drives for This PC and the navigation pane: every drive letter Windows has,
//! with a state for the ones that can't be read right now (an empty card
//! reader or DVD drive, a network drive that isn't connected, a volume that
//! doesn't answer) instead of leaving them out. Device changes are watched
//! so the lists follow a USB drive being plugged in or a share being mapped.
use crate::fs_list::wide;
use serde::Serialize;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};

#[derive(Serialize, Clone, Debug)]
pub struct Drive {
    pub path: String,
    /// As Explorer names it: "OS (C:)", "DVD RW Drive (E:)", "share (\\server) (Z:)".
    pub name: String,
    /// "fixed", "removable", "optical", "network" or "ram".
    pub kind: &'static str,
    pub total: u64,
    pub free: u64,
    /// "ready"; "empty" (no disc or card in it); "offline" (a network drive
    /// that isn't connected); "slow" (didn't answer in time); "error".
    pub state: &'static str,
    /// Windows' words for why it can't be read, when it can't.
    pub note: Option<String>,
    pub read_only: bool,
    /// Removable media, discs and USB disks: Explorer offers Eject for them.
    pub ejectable: bool,
    /// The share a network drive maps (\\server\share).
    pub remote: Option<String>,
}

/// How long one drive may take before it's shown as not responding. Each
/// drive is asked on its own thread, so one that hangs (a share that's gone,
/// a disc spinning up) holds up only itself.
const PATIENCE: Duration = Duration::from_millis(1500);

pub fn list() -> Vec<Drive> {
    let mask = unsafe { GetLogicalDrives() };
    let asked: Vec<(char, mpsc::Receiver<Drive>)> = (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| {
            let letter = (b'A' + i) as char;
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                unsafe { let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED); }
                let _ = tx.send(query(letter));
            });
            (letter, rx)
        })
        .collect();
    let deadline = Instant::now() + PATIENCE;
    asked
        .into_iter()
        .map(|(letter, rx)| match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(d) => d,
            Err(_) => Drive {
                path: format!("{letter}:\\"),
                name: format!("({letter}:)"),
                kind: "fixed",
                total: 0,
                free: 0,
                state: "slow",
                note: Some("This drive isn't responding.".into()),
                read_only: false,
                ejectable: false,
                remote: None,
            },
        })
        .collect()
}

fn query(letter: char) -> Drive {
    use windows::Win32::Foundation::ERROR_NOT_READY;
    let root = format!("{letter}:\\");
    let w = wide(&root);
    let kind = match unsafe { GetDriveTypeW(PCWSTR(w.as_ptr())) } {
        2 => "removable",
        4 => "network",
        5 => "optical",
        6 => "ram",
        // Fixed, and letters whose root can't be found (a substituted folder
        // that was deleted): those stay listed with Windows' reason.
        _ => "fixed",
    };
    let mut d = Drive {
        path: root.clone(),
        name: format!("({letter}:)"),
        kind,
        total: 0,
        free: 0,
        state: "ready",
        note: None,
        read_only: false,
        ejectable: kind == "removable" || kind == "optical",
        remote: None,
    };
    if kind == "network" {
        let (remote, connected) = net_target(letter);
        d.remote = remote;
        // Windows remembers it but it isn't connected: say so without
        // touching the network, which could take half a minute to give up.
        if !connected {
            d.state = "offline";
            d.note = Some("Disconnected".into());
            d.name = net_name(d.remote.as_deref(), letter);
            return d;
        }
    }
    let (mut total, mut free) = (0u64, 0u64);
    let space = unsafe { GetDiskFreeSpaceExW(PCWSTR(w.as_ptr()), None, Some(&mut total), Some(&mut free)) };
    let mut flags = 0u32;
    let _ = unsafe { GetVolumeInformationW(PCWSTR(w.as_ptr()), None, None, None, Some(&mut flags), None) };
    match space {
        Ok(()) => (d.total, d.free) = (total, free),
        Err(e) if e.code() == ERROR_NOT_READY.to_hresult() => d.state = "empty",
        Err(e) => {
            d.state = "error";
            d.note = Some(e.message().trim().to_string());
        }
    }
    const FILE_READ_ONLY_VOLUME: u32 = 0x0008_0000;
    d.read_only = flags & FILE_READ_ONLY_VOLUME != 0;
    d.ejectable |= kind == "fixed" && on_usb(letter);
    d.name = display_name(&root).unwrap_or_else(|| if kind == "network" { net_name(d.remote.as_deref(), letter) } else { d.name.clone() });
    d
}

/// The drive's name as Explorer shows it.
fn display_name(root: &str) -> Option<String> {
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_DISPLAYNAME};
    let w = wide(root);
    let mut info = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(PCWSTR(w.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut info), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_DISPLAYNAME)
    };
    let len = info.szDisplayName.iter().position(|&c| c == 0).unwrap_or(0);
    (ok != 0 && len > 0).then(|| String::from_utf16_lossy(&info.szDisplayName[..len]))
}

/// "share (\\server) (Z:)", Explorer's name for a mapped drive.
fn net_name(remote: Option<&str>, letter: char) -> String {
    match remote.and_then(|r| r.trim_start_matches('\\').split_once('\\')) {
        Some((server, share)) => format!("{share} (\\\\{server}) ({letter}:)"),
        None => format!("Network Drive ({letter}:)"),
    }
}

/// The share a drive letter maps, and whether it's connected now.
fn net_target(letter: char) -> (Option<String>, bool) {
    use windows::Win32::Foundation::{ERROR_CONNECTION_UNAVAIL, NO_ERROR};
    use windows::Win32::NetworkManagement::WNet::WNetGetConnectionW;
    let local = wide(&format!("{letter}:"));
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let r = unsafe { WNetGetConnectionW(PCWSTR(local.as_ptr()), Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len) };
    let text = || {
        let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
        (n > 0).then(|| String::from_utf16_lossy(&buf[..n]))
    };
    if r == NO_ERROR {
        (text(), true)
    } else if r == ERROR_CONNECTION_UNAVAIL {
        (text(), false)
    } else {
        (None, true)
    }
}

/// Whether a fixed drive is a disk on USB (an external drive Explorer can eject).
fn on_usb(letter: char) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows::Win32::Storage::FileSystem::BusTypeUsb;
    use windows::Win32::System::Ioctl::{
        PropertyStandardQuery, StorageDeviceProperty, IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
    };
    use windows::Win32::System::IO::DeviceIoControl;
    let dev = wide(&format!(r"\\.\{letter}:"));
    unsafe {
        // No access rights needed to ask a device about itself.
        let Ok(h) = CreateFileW(PCWSTR(dev.as_ptr()), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) else {
            return false;
        };
        let q = STORAGE_PROPERTY_QUERY { PropertyId: StorageDeviceProperty, QueryType: PropertyStandardQuery, ..Default::default() };
        let mut desc = STORAGE_DEVICE_DESCRIPTOR::default();
        let mut got = 0u32;
        let ok = DeviceIoControl(
            h,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&q as *const _ as *const _),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(&mut desc as *mut _ as *mut _),
            std::mem::size_of::<STORAGE_DEVICE_DESCRIPTOR>() as u32,
            Some(&mut got),
            None,
        )
        .is_ok();
        let _ = CloseHandle(h);
        ok && desc.BusType == BusTypeUsb
    }
}

/// Tell the page when drives come and go: a USB drive plugged in or pulled
/// out, a disc inserted, a network drive mapped or disconnected. A hidden
/// top-level window gets Windows' device broadcasts (message-only windows
/// don't) and the shell's drive notifications.
pub fn watch(app: AppHandle) {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        SHChangeNotifyEntry, SHChangeNotifyRegister, SHCNE_DRIVEADD, SHCNE_DRIVEREMOVED, SHCNE_MEDIAINSERTED, SHCNE_MEDIAREMOVED,
        SHCNE_NETSHARE, SHCNE_NETUNSHARE, SHCNE_SERVERDISCONNECT, SHCNRF_InterruptLevel, SHCNRF_ShellLevel,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, RegisterClassW, SetTimer, MSG, WINDOW_EX_STYLE,
        WM_APP, WM_DEVICECHANGE, WM_TIMER, WNDCLASSW, WS_POPUP,
    };
    use windows::core::w;
    static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();
    let _ = APP.set(app);
    const SHELL: u32 = WM_APP + 1;
    const SETTLE: usize = 1;
    unsafe extern "system" fn proc(h: windows::Win32::Foundation::HWND, m: u32, wp: windows::Win32::Foundation::WPARAM, lp: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::LRESULT {
        match m {
            // A burst of changes (a USB disk brings several) becomes one event.
            WM_DEVICECHANGE | SHELL => {
                unsafe { SetTimer(Some(h), SETTLE, 400, None) };
                windows::Win32::Foundation::LRESULT(1)
            }
            WM_TIMER if wp.0 == SETTLE => {
                unsafe { let _ = KillTimer(Some(h), SETTLE); }
                if let Some(app) = APP.get() {
                    let _ = app.emit("drives-changed", ());
                }
                windows::Win32::Foundation::LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(h, m, wp, lp) },
        }
    }
    std::thread::spawn(|| unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        let inst = GetModuleHandleW(None).unwrap_or_default();
        let class = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: inst.into(), lpszClassName: w!("FilezyDevices"), ..Default::default() };
        RegisterClassW(&class);
        let Ok(h) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("FilezyDevices"), w!(""), WS_POPUP, 0, 0, 0, 0, None, None, Some(inst.into()), None) else {
            return;
        };
        // The desktop (an empty ID list), and everything under it.
        let empty = [0u8; 2];
        let entry = SHChangeNotifyEntry { pidl: empty.as_ptr() as *mut _, fRecursive: true.into() };
        let events = SHCNE_DRIVEADD | SHCNE_DRIVEREMOVED | SHCNE_MEDIAINSERTED | SHCNE_MEDIAREMOVED | SHCNE_NETSHARE | SHCNE_NETUNSHARE | SHCNE_SERVERDISCONNECT;
        SHChangeNotifyRegister(h, SHCNRF_InterruptLevel | SHCNRF_ShellLevel, events.0 as i32, SHELL, 1, &entry);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    });
}

/// The folders a computer shares ("\\server" as a place): its disk
/// shares, without the hidden ones whose names end in "$", as Explorer lists
/// them. Windows signs in to the computer as needed.
pub fn shares(server: &str) -> Result<Vec<crate::listing::Entry>, String> {
    use windows::Win32::Foundation::WIN32_ERROR;
    use windows::Win32::NetworkManagement::NetManagement::{NetApiBufferFree, MAX_PREFERRED_LENGTH};
    use windows::Win32::Storage::FileSystem::{NetShareEnum, SHARE_INFO_1};
    const SPECIAL: u32 = 0x8000_0000;
    let w = wide(server.trim_end_matches('\\'));
    let (mut buf, mut read, mut total) = (std::ptr::null_mut::<u8>(), 0u32, 0u32);
    let r = unsafe { NetShareEnum(PCWSTR(w.as_ptr()), 1, &mut buf, MAX_PREFERRED_LENGTH, &mut read, &mut total, None) };
    if r != 0 {
        return Err(windows::core::Error::from(WIN32_ERROR(r).to_hresult()).message().trim().to_string());
    }
    let items = if buf.is_null() { &[][..] } else { unsafe { std::slice::from_raw_parts(buf as *const SHARE_INFO_1, read as usize) } };
    let out = items
        .iter()
        .filter(|s| s.shi1_type.0 & 0xff == 0 && s.shi1_type.0 & SPECIAL == 0)
        .filter_map(|s| unsafe { s.shi1_netname.to_string() }.ok())
        .filter(|n| !n.ends_with('$'))
        .map(|name| crate::listing::Entry {
            name,
            is_dir: true,
            size: 0,
            modified: 0,
            created: 0,
            ext: String::new(),
            attrs: crate::listing::ATTR_DIRECTORY,
            parent: None,
        })
        .collect();
    unsafe { NetApiBufferFree(Some(buf as *const _)) };
    Ok(out)
}

#[derive(Serialize, Clone, Debug)]
pub struct Place {
    pub name: String,
    /// "\\NAME", which Filezy opens as that computer's shares.
    pub path: String,
}

/// The computers Windows' Network folder finds. Discovery can take a while
/// and finds nothing when network discovery is off; what has turned up after
/// `wait` is returned. Only computers with a "\\NAME" path are kept, as
/// those are what Filezy can open.
pub fn network(wait: Duration) -> Vec<Place> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || unsafe {
        use windows::Win32::UI::Shell::{
            BHID_EnumItems, FOLDERID_NetworkFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem, KF_FLAG_DEFAULT,
            SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY,
        };
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        let Ok(net) = SHGetKnownFolderItem::<IShellItem>(&FOLDERID_NetworkFolder, KF_FLAG_DEFAULT, None) else { return };
        let Ok(items) = net.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else { return };
        let text = |p: windows::core::PWSTR| {
            let s = p.to_string().unwrap_or_default();
            windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
            s
        };
        loop {
            let mut got = [None];
            if items.Next(&mut got, None).is_err() {
                break;
            }
            let Some(item) = got[0].take() else { break };
            let path = item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING).map(text).unwrap_or_default();
            if !path.starts_with("\\\\") {
                continue;
            }
            let name = item.GetDisplayName(SIGDN_NORMALDISPLAY).map(text).unwrap_or_else(|_| path.clone());
            if tx.send(Place { name, path }).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + wait;
    let mut out = Vec::new();
    while let Ok(p) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        out.push(p);
    }
    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_drive_answers_or_is_marked() {
        let t0 = Instant::now();
        let all = list();
        assert!(t0.elapsed() < PATIENCE + Duration::from_millis(500), "listing waited on a slow drive");
        let c = all.iter().find(|d| d.path == "C:\\").expect("C: is listed");
        assert_eq!((c.kind, c.state), ("fixed", "ready"));
        assert!(c.total > 0 && c.name.ends_with("(C:)"), "{c:?}");
    }

    #[test]
    fn this_computer_lists_its_shares_without_the_hidden_ones() {
        let found = shares(r"\\localhost").expect("this computer answers");
        let names: Vec<&str> = found.iter().map(|e| e.name.as_str()).collect();
        assert!(found.iter().all(|e| e.is_dir) && names.iter().all(|n| !n.ends_with('$')), "{names:?}");
        assert!(shares(r"\\no-such-computer-filezy").is_err());
    }

    #[test]
    fn mapped_drives_read_like_explorer() {
        assert_eq!(net_name(Some(r"\\server\share"), 'Z'), r"share (\\server) (Z:)");
        assert_eq!(net_name(None, 'Y'), "Network Drive (Y:)");
    }
}
