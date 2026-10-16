//! A stand-in cloud provider for the E2E cloud suite (scripts/e2e/suites/cloud.mjs).
//! It registers a sync root in a folder it's given, fills it with placeholders
//! in each state Explorer tells apart, and serves their downloads until told to
//! quit; then it disconnects and unregisters. Nothing outside that folder is touched.
//!
//!   testcloud start <id> <folder>   register, populate, serve; stdin takes commands
//!   testcloud clean <prefix>        unregister every sync root whose id starts with <prefix>
//!
//! Like a real provider it acts on pin changes: a file set to Always keep on
//! this device is downloaded, and one set to Free up space is emptied.
//!
//! Commands on stdin, one per line: "fail <name>" makes that file's downloads
//! fail, and "fix <name>" lets them succeed again; "slow <name>" makes them
//! take two seconds; "insync <name>" marks a
//! changed file uploaded; "quit" cleans up and exits. Each answers "ok".
use std::collections::HashSet;
use std::io::BufRead;
use std::os::windows::io::AsRawHandle;
use std::sync::Mutex;
use windows::core::{HSTRING, PCWSTR};
use windows::Storage::Provider::*;
use windows::Storage::StorageFolder;
use windows::Storage::Streams::DataWriter;
use windows::Win32::Foundation::{HANDLE, NTSTATUS};
use windows::Win32::Storage::CloudFilters::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

/// Every file is this long, so a download is one transfer that ends at EOF.
const SIZE: usize = 5000;

/// Names whose downloads fail, and names whose downloads are slow.
static FAIL: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static SLOW: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn has(set: &Mutex<Option<HashSet<String>>>, name: &str) -> bool {
    set.lock().unwrap().as_ref().is_some_and(|s| s.contains(name))
}

/// A file's bytes: its name, repeated.
fn content(name: &str) -> Vec<u8> {
    format!("{name}\n").bytes().cycle().take(SIZE).collect()
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// The platform asks for a file's data: send it, or fail the request.
unsafe extern "system" fn fetch(info: *const CF_CALLBACK_INFO, params: *const CF_CALLBACK_PARAMETERS) {
    let info = &*info;
    let fetch = (*params).Anonymous.FetchData;
    // The identity given at creation is the file's name.
    let name = String::from_utf8_lossy(std::slice::from_raw_parts(info.FileIdentity as *const u8, info.FileIdentityLength as usize)).to_string();
    if has(&SLOW, &name) {
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    let data = content(&name);
    let off = fetch.RequiredFileOffset as usize;
    let op = CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: CF_OPERATION_TYPE_TRANSFER_DATA,
        ConnectionKey: info.ConnectionKey,
        TransferKey: info.TransferKey,
        CorrelationVector: info.CorrelationVector,
        RequestKey: info.RequestKey,
        ..Default::default()
    };
    let mut p = CF_OPERATION_PARAMETERS { ParamSize: size_of::<CF_OPERATION_PARAMETERS>() as u32, ..Default::default() };
    let t = &mut p.Anonymous.TransferData;
    if has(&FAIL, &name) {
        // STATUS_CLOUD_FILE_UNSUCCESSFUL: the provider couldn't get the file.
        t.CompletionStatus = NTSTATUS(0xC000_CF0Bu32 as i32);
        t.Offset = fetch.RequiredFileOffset;
        t.Length = fetch.RequiredLength;
    } else {
        t.Buffer = data[off..].as_ptr() as *const _;
        t.Offset = off as i64;
        t.Length = (data.len() - off) as i64;
    }
    if let Err(e) = CfExecute(&op, &mut p) {
        eprintln!("transfer of {name} failed: {e}");
    }
}

fn register(id: &str, folder: &str) -> windows::core::Result<()> {
    let info = StorageProviderSyncRootInfo::new()?;
    info.SetId(&HSTRING::from(id))?;
    info.SetPath(&StorageFolder::GetFolderFromPathAsync(&HSTRING::from(folder))?.join()?)?;
    info.SetDisplayNameResource(&HSTRING::from("Filezy test cloud"))?;
    info.SetIconResource(&HSTRING::from(r"%SystemRoot%\system32\imageres.dll,-1043"))?;
    info.SetHydrationPolicy(StorageProviderHydrationPolicy::Full)?;
    info.SetHydrationPolicyModifier(StorageProviderHydrationPolicyModifier::AutoDehydrationAllowed)?;
    info.SetPopulationPolicy(StorageProviderPopulationPolicy::AlwaysFull)?;
    info.SetInSyncPolicy(StorageProviderInSyncPolicy::Default)?;
    info.SetHardlinkPolicy(StorageProviderHardlinkPolicy::None)?;
    info.SetShowSiblingsAsGroup(false)?;
    info.SetVersion(&HSTRING::from("1"))?;
    let w = DataWriter::new()?;
    w.WriteString(&HSTRING::from(id))?;
    info.SetContext(&w.DetachBuffer()?)?;
    StorageProviderSyncRootManager::Register(&info)
}

/// Placeholders for `names` in `dir`, marked in sync and not yet downloaded.
fn placeholders(dir: &str, names: &[&str], folder: bool) -> windows::core::Result<()> {
    let now = 133_000_000_000_000_000i64; // a fixed time in 2022, as a FILETIME
    let ws: Vec<Vec<u16>> = names.iter().map(|n| wide(n)).collect();
    let mut infos: Vec<CF_PLACEHOLDER_CREATE_INFO> = names
        .iter()
        .zip(&ws)
        .map(|(n, w)| {
            let mut i = CF_PLACEHOLDER_CREATE_INFO {
                RelativeFileName: PCWSTR(w.as_ptr()),
                FileIdentity: n.as_ptr() as *const _,
                FileIdentityLength: n.len() as u32,
                Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC,
                ..Default::default()
            };
            let b = &mut i.FsMetadata.BasicInfo;
            (b.CreationTime, b.LastAccessTime, b.LastWriteTime, b.ChangeTime) = (now, now, now, now);
            if folder {
                b.FileAttributes = 0x10; // FILE_ATTRIBUTE_DIRECTORY
                i.Flags |= CF_PLACEHOLDER_CREATE_FLAG_DISABLE_ON_DEMAND_POPULATION;
            } else {
                b.FileAttributes = 0x80; // FILE_ATTRIBUTE_NORMAL
                i.FsMetadata.FileSize = SIZE as i64;
            }
            i
        })
        .collect();
    unsafe { CfCreatePlaceholders(PCWSTR(wide(dir).as_ptr()), &mut infos, CF_CREATE_FLAG_NONE, None) }
}

/// Opens a file without reading it, for changing its cloud state. Opening
/// alone never makes the platform download it; only reading the data does.
fn handle(path: &str) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().read(true).write(true).open(path)
}

const PINNED: u32 = 0x8_0000;
const UNPINNED: u32 = 0x10_0000;
const ONLINE_ONLY: u32 = 0x40_0000; // FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS

/// What a provider does when someone changes a file's pin state: download a
/// kept file, empty a freed one. Errors are reported and the watch goes on.
fn follow_pin(path: &std::path::Path) {
    use std::os::windows::fs::MetadataExt;
    let Ok(m) = std::fs::metadata(path) else { return };
    let a = m.file_attributes();
    if m.is_dir() {
        return;
    }
    let r = if a & PINNED != 0 && a & ONLINE_ONLY != 0 {
        std::fs::read(path).map(|_| ()).map_err(|e| e.to_string())
    } else if a & UNPINNED != 0 && a & ONLINE_ONLY == 0 {
        handle(&path.to_string_lossy()).map_err(|e| e.to_string()).and_then(|f| {
            unsafe { CfDehydratePlaceholder(HANDLE(f.as_raw_handle()), 0, -1, CF_DEHYDRATE_FLAG_NONE, None) }.map_err(|e| e.to_string())
        })
    } else {
        return;
    };
    if let Err(e) = r {
        eprintln!("{}: {e}", path.display());
    }
}

fn err(what: &'static str) -> impl Fn(windows::core::Error) -> String {
    move |e| format!("{what}: {e}")
}

fn start(id: &str, folder: &str) -> Result<(), String> {
    let root = std::path::Path::new(folder);
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    register(id, folder).map_err(err("register"))?;
    placeholders(folder, &["online.txt", "local.txt", "pinned.txt", "pending.txt", "broken.txt", "slow.txt"], false).map_err(err("create files"))?;
    placeholders(folder, &["docs"], true).map_err(err("create folder"))?;
    placeholders(&root.join("docs").to_string_lossy(), &["inner.txt"], false).map_err(err("create inner file"))?;
    let table = [
        CF_CALLBACK_REGISTRATION { Type: CF_CALLBACK_TYPE_FETCH_DATA, Callback: Some(fetch) },
        CF_CALLBACK_REGISTRATION { Type: CF_CALLBACK_TYPE_NONE, Callback: None },
    ];
    let key = unsafe { CfConnectSyncRoot(PCWSTR(wide(folder).as_ptr()), table.as_ptr(), None, CF_CONNECT_FLAG_NONE) }.map_err(err("connect"))?;
    *FAIL.lock().unwrap() = Some(HashSet::from(["broken.txt".to_string()]));
    *SLOW.lock().unwrap() = Some(HashSet::from(["slow.txt".to_string()]));
    let at = |n: &str| root.join(n).to_string_lossy().to_string();
    // Downloaded, then: kept as is, pinned, or changed here and not yet uploaded.
    for n in ["local.txt", "pinned.txt", "pending.txt"] {
        std::fs::read(at(n)).map_err(|e| format!("download {n}: {e}"))?;
    }
    let pinned = handle(&at("pinned.txt")).map_err(|e| e.to_string())?;
    unsafe { CfSetPinState(HANDLE(pinned.as_raw_handle()), CF_PIN_STATE_PINNED, CF_SET_PIN_FLAG_NONE, None) }.map_err(err("pin"))?;
    let pending = handle(&at("pending.txt")).map_err(|e| e.to_string())?;
    unsafe { CfSetInSyncState(HANDLE(pending.as_raw_handle()), CF_IN_SYNC_STATE_NOT_IN_SYNC, CF_SET_IN_SYNC_FLAG_NONE, None) }.map_err(err("mark not in sync"))?;
    drop((pinned, pending));
    use notify::Watcher;
    let mut watcher = notify::recommended_watcher(|ev: notify::Result<notify::Event>| {
        ev.into_iter().flat_map(|e| e.paths).for_each(|p| follow_pin(&p));
    })
    .map_err(|e| e.to_string())?;
    watcher.watch(root, notify::RecursiveMode::Recursive).map_err(|e| e.to_string())?;
    println!("ready");
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap_or_default();
        let (cmd, name) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        match cmd {
            "fail" => drop(FAIL.lock().unwrap().get_or_insert_default().insert(name.to_string())),
            "slow" => drop(SLOW.lock().unwrap().get_or_insert_default().insert(name.to_string())),
            "fix" => drop(FAIL.lock().unwrap().get_or_insert_default().remove(name)),
            "insync" => {
                let f = handle(&at(name)).map_err(|e| e.to_string())?;
                unsafe { CfSetInSyncState(HANDLE(f.as_raw_handle()), CF_IN_SYNC_STATE_IN_SYNC, CF_SET_IN_SYNC_FLAG_NONE, None) }.map_err(err("mark in sync"))?;
            }
            "quit" => break,
            _ => {}
        };
        println!("ok");
    }
    drop(watcher);
    unsafe { CfDisconnectSyncRoot(key) }.map_err(err("disconnect"))?;
    StorageProviderSyncRootManager::Unregister(&HSTRING::from(id)).map_err(err("unregister"))
}

/// Unregisters sync roots a crashed run left behind.
fn clean(prefix: &str) -> Result<(), String> {
    let roots = StorageProviderSyncRootManager::GetCurrentSyncRoots().map_err(|e| e.to_string())?;
    for r in roots {
        let id = r.Id().map_err(|e| e.to_string())?.to_string();
        if id.starts_with(prefix) {
            StorageProviderSyncRootManager::Unregister(&HSTRING::from(&id)).map_err(|e| format!("unregister {id}: {e}"))?;
            println!("unregistered {id}");
        }
    }
    Ok(())
}

fn main() {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().expect("COM") };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["start", id, folder] => start(id, folder),
        ["clean", prefix] => clean(prefix),
        _ => Err("usage: testcloud start <id> <folder> | testcloud clean <prefix>".into()),
    };
    if let Err(e) = r {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
