//! What the preview pane shows for a file.
//!
//! The page draws text, images, audio, video and PDF itself. Text is decoded
//! here the way Notepad decodes it: a byte-order mark, else UTF-16 without
//! one, else UTF-8, else the ANSI code page. Media and PDF come through the
//! `preview` scheme (`serve`), which serves only the file shown last. PDF
//! uses WebView2's own viewer, because Edge's PDF previewer draws a blank page
//! when anything but Explorer hosts it.
//!
//! Everything else Windows can preview (Word, Excel, PowerPoint, fonts, mail)
//! goes to that file type's own preview handler, hosted as Explorer
//! hosts it: in prevhost.exe, never in this process. It draws into a window of
//! ours that the page keeps over the preview pane. That window is an owned
//! popup, not a child: WebView2 composites the page above every child window,
//! so a child would never be seen.
use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::AppHandle;
use windows::core::{w, Interface, GUID, HSTRING, PWSTR};
use windows::Win32::Foundation::POINT;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, HDC};
use windows::Win32::System::Com::{CLSIDFromString, CoCreateInstance, CLSCTX_LOCAL_SERVER, STGM_READ, STGM_SHARE_DENY_NONE};
use windows::Win32::UI::Shell::Common::{PERCEIVED, PERCEIVED_TYPE_IMAGE, PERCEIVED_TYPE_TEXT, PERCEIVED_TYPE_VIDEO};
use windows::Win32::UI::Shell::PropertiesSystem::{IInitializeWithFile, IInitializeWithStream};
use windows::Win32::UI::Shell::{
    AssocGetPerceivedType, AssocQueryStringW, IInitializeWithItem, IPreviewHandler, IPreviewHandlerVisuals, IShellItem,
    SHCreateItemFromParsingName, SHCreateStreamOnFileEx, ASSOCF_NONE, ASSOCSTR_SHELLEXTENSION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetWindow, LoadCursorW, PeekMessageW, RegisterClassW, SetWindowPos,
    ShowWindow, TranslateMessage, GW_HWNDPREV, HWND_TOP, IDC_ARROW, MA_NOACTIVATE, MSG, PM_REMOVE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, WM_ERASEBKGND, WM_MOUSEACTIVATE, WNDCLASSW, WS_CLIPCHILDREN, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_POPUP,
};

/// Images WebView2 decodes itself; other images show as Windows' thumbnail.
const IMAGES: &[&str] = &["png", "jpg", "jpeg", "jfif", "gif", "bmp", "webp", "svg", "ico", "avif"];
/// Audio and video WebView2 plays itself.
const AUDIO: &[&str] = &["mp3", "wav", "ogg", "oga", "opus", "m4a", "aac", "flac", "weba"];
const VIDEO: &[&str] = &["mp4", "m4v", "webm", "ogv", "mov"];
/// Text the page shows even when Windows doesn't call it text.
const TEXT: &[&str] = &[
    "txt",
    "md",
    "markdown",
    "log",
    "json",
    "jsonc",
    "xml",
    "csv",
    "tsv",
    "ini",
    "cfg",
    "conf",
    "toml",
    "yaml",
    "yml",
    "js",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "jsx",
    "rs",
    "py",
    "css",
    "scss",
    "less",
    "svelte",
    "vue",
    "bat",
    "cmd",
    "ps1",
    "psm1",
    "sh",
    "c",
    "h",
    "cpp",
    "hpp",
    "cc",
    "cs",
    "java",
    "kt",
    "go",
    "rb",
    "php",
    "lua",
    "sql",
    "gitignore",
    "gitattributes",
    "editorconfig",
    "env",
    "properties",
    "reg",
    "inf",
    "diff",
    "patch",
    "tex",
    "srt",
    "vtt",
];
/// Windows' own plain-text previewer: the page's text view does its job.
const TXT_PREVIEWER: &str = "{1531d583-8375-4d3f-b5fb-d23bbd169f22}";
/// The shell's key for preview handlers (IPreviewHandler's IID).
const PREVIEW_HANDLER_KEY: windows::core::PCWSTR = w!("{8895b1c6-b41f-4c1c-a562-0d564250836f}");
/// Text past this is left out, and the page says so.
const TEXT_MAX: usize = 256 * 1024;

const ATTR_OFFLINE: u32 = 0x1000;
const ATTR_RECALL_ON_OPEN: u32 = 0x4_0000;
const ATTR_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    /// "text", "image", "thumb" (an image or video only Windows can decode,
    /// shown as its thumbnail), "audio", "video", "pdf", "native" or "none".
    kind: &'static str,
    /// A cloud file that isn't on this device; reading it downloads it.
    online_only: bool,
    /// Downloaded from the internet (Mark of the Web). Windows won't run a
    /// preview handler on such a file, and Filezy won't, nor open it as a PDF.
    untrusted: bool,
}

/// The one file the `preview` scheme serves: the media or PDF shown last.
static SERVED: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Decide how to preview `path`. Media and PDF it returns become the file the
/// `preview` scheme serves, in place of the one before. `download` says the user agreed to download an online-only file:
/// it is downloaded here, before anything reads it, so a failure says why.
pub fn info(path: &str, download: bool) -> Result<Info, String> {
    *SERVED.lock().unwrap() = None;
    let native = paths::to_native(path);
    let attrs = unsafe { windows::Win32::Storage::FileSystem::GetFileAttributesW(&HSTRING::from(&native)) };
    let mut online_only = attrs != u32::MAX && attrs & (ATTR_OFFLINE | ATTR_RECALL_ON_OPEN | ATTR_RECALL_ON_DATA_ACCESS) != 0;
    if online_only && download {
        hydrate(&native)?;
        online_only = false;
    }
    let readable = !online_only;
    let ext = ext_of(path);
    let perceived = perceived(&ext);
    let kind = if IMAGES.contains(&ext.as_str()) {
        "image"
    } else if AUDIO.contains(&ext.as_str()) {
        "audio"
    } else if VIDEO.contains(&ext.as_str()) {
        "video"
    } else if ext == "pdf" {
        // Only a real PDF: the asset protocol serves a file without PDF's
        // signature as a web page, whose scripts mustn't run in the pane.
        if !readable || starts_with(&native, b"%PDF") {
            "pdf"
        } else {
            "none"
        }
    } else if handler(&ext).is_some() {
        "native"
    } else if TEXT.contains(&ext.as_str()) || perceived == PERCEIVED_TYPE_TEXT {
        "text"
    } else if perceived == PERCEIVED_TYPE_IMAGE || perceived == PERCEIVED_TYPE_VIDEO {
        "thumb"
    } else if readable && looks_like_text(&native) {
        // No extension Windows knows (LICENSE, Makefile, .nfo): text if it reads as text.
        "text"
    } else {
        "none"
    };
    if readable && matches!(kind, "image" | "audio" | "video" | "pdf") {
        // Requests are compared by canonical path, so another spelling of the
        // same file is served and nothing else is.
        *SERVED.lock().unwrap() = std::fs::canonicalize(&native).ok();
    }
    Ok(Info { kind, online_only, untrusted: matches!(kind, "native" | "pdf") && zone(&native) >= 3 })
}

/// Download an online-only cloud file and wait until it is on this device.
/// A file that is offline for another reason (not a cloud placeholder) is
/// left for the read itself to bring back.
fn hydrate(native: &str) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{ERROR_NOT_A_CLOUD_FILE, HANDLE};
    use windows::Win32::Storage::CloudFilters::{CfHydratePlaceholder, CF_HYDRATE_FLAG_NONE};
    let f = std::fs::File::open(native).map_err(|e| e.to_string())?;
    match unsafe { CfHydratePlaceholder(HANDLE(f.as_raw_handle()), 0, -1, CF_HYDRATE_FLAG_NONE, None) } {
        Err(e) if e.code() != ERROR_NOT_A_CLOUD_FILE.to_hresult() => Err(format!("This file couldn't be downloaded. {}", e.message())),
        _ => Ok(()),
    }
}

fn ext_of(path: &str) -> String {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    name.rfind('.').map(|i| name[i + 1..].to_lowercase()).unwrap_or_default()
}

fn perceived(ext: &str) -> PERCEIVED {
    let mut p = PERCEIVED(0);
    if !ext.is_empty() {
        let mut flags = 0u32;
        let _ = unsafe { AssocGetPerceivedType(&HSTRING::from(format!(".{ext}")), &mut p, &mut flags, None) };
    }
    p
}

/// The preview handler registered for an extension, unless it's Windows' plain-text one.
fn handler(ext: &str) -> Option<GUID> {
    if ext.is_empty() {
        return None;
    }
    let mut buf = [0u16; 64];
    let mut n = buf.len() as u32;
    let hr = unsafe {
        AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_SHELLEXTENSION,
            &HSTRING::from(format!(".{ext}")),
            PREVIEW_HANDLER_KEY,
            Some(PWSTR(buf.as_mut_ptr())),
            &mut n,
        )
    };
    if hr.is_err() || n < 2 {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..n as usize - 1]);
    if s.eq_ignore_ascii_case(TXT_PREVIEWER) {
        return None;
    }
    unsafe { CLSIDFromString(&HSTRING::from(s)) }.ok()
}

/// The security zone a download was saved from (3 is the internet), or 0.
fn zone(native: &str) -> u32 {
    std::fs::read_to_string(format!("{native}:Zone.Identifier"))
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.trim().strip_prefix("ZoneId=")?.trim().parse().ok()))
        .unwrap_or(0)
}

fn starts_with(native: &str, magic: &[u8]) -> bool {
    use std::io::Read;
    let mut buf = vec![0u8; magic.len()];
    std::fs::File::open(native).and_then(|mut f| f.read_exact(&mut buf)).is_ok() && buf == magic
}

fn looks_like_text(native: &str) -> bool {
    use std::io::Read;
    let mut buf = vec![0u8; 4096];
    let Ok(mut f) = std::fs::File::open(native) else { return false };
    let Ok(n) = f.read(&mut buf) else { return false };
    n > 0 && decode(&buf[..n], n == buf.len()).is_some()
}

#[derive(Serialize)]
pub struct Text {
    text: String,
    /// Only the first TEXT_MAX bytes are in `text`.
    truncated: bool,
}

/// The start of a text file, decoded; None when it isn't text.
pub fn text(path: &str) -> Result<Option<Text>, String> {
    use std::io::Read;
    let f = std::fs::File::open(paths::to_native(path)).map_err(|e| e.to_string())?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut buf = Vec::with_capacity(TEXT_MAX.min(len as usize));
    f.take(TEXT_MAX as u64).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let truncated = len > buf.len() as u64;
    Ok(decode(&buf, truncated).map(|text| Text { text, truncated }))
}

/// Decode text as Notepad does. `cut` says the bytes stop mid-file, so a
/// character split at the end isn't a sign of binary data.
fn decode(b: &[u8], cut: bool) -> Option<String> {
    let utf16 = |b: &[u8], le: bool| {
        let units = b.chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) });
        char::decode_utf16(units).map(|c| c.unwrap_or('\u{FFFD}')).collect::<String>()
    };
    if let Some(rest) = b.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Some(utf8(rest, cut));
    }
    if let Some(rest) = b.strip_prefix(&[0xFF, 0xFE]) {
        return Some(utf16(rest, true));
    }
    if let Some(rest) = b.strip_prefix(&[0xFE, 0xFF]) {
        return Some(utf16(rest, false));
    }
    // UTF-16 without a mark: mostly-Latin text has a zero in every other byte.
    let pairs = b.len() / 2;
    if pairs >= 2 {
        let zeros = |start: usize| b.iter().skip(start).step_by(2).take(pairs).filter(|&&c| c == 0).count();
        let (even, odd) = (zeros(0), zeros(1));
        if odd * 2 > pairs && even * 20 < pairs {
            return Some(utf16(b, true));
        }
        if even * 2 > pairs && odd * 20 < pairs {
            return Some(utf16(b, false));
        }
    }
    // Binary: a zero byte, or many control characters other than tab, line breaks, form feed and escape.
    let sample = &b[..b.len().min(4096)];
    let controls = sample.iter().filter(|&&c| c < 0x20 && !matches!(c, b'\t' | b'\n' | b'\r' | 0x0C | 0x1B)).count();
    if sample.contains(&0) || controls * 20 > sample.len() {
        return None;
    }
    match std::str::from_utf8(b) {
        Ok(s) => Some(s.to_string()),
        // Only the last character was cut in two.
        Err(e) if cut && e.error_len().is_none() => Some(utf8(b, cut)),
        Err(_) => Some(ansi(b)),
    }
}

fn utf8(b: &[u8], cut: bool) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(e) if cut && e.error_len().is_none() => String::from_utf8_lossy(&b[..e.valid_up_to()]).into_owned(),
        Err(_) => String::from_utf8_lossy(b).into_owned(),
    }
}

/// This computer's ANSI code page, as Notepad reads a file that isn't UTF-8.
fn ansi(b: &[u8]) -> String {
    use windows::Win32::Globalization::{MultiByteToWideChar, CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS};
    let mut buf = vec![0u16; b.len() + 1];
    let n = unsafe { MultiByteToWideChar(CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), b, Some(&mut buf)) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

// ---- Native preview handlers ----

/// Where the preview goes, in physical pixels of the window's client area.
#[derive(Deserialize, Clone, Copy)]
pub struct Place {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

enum Req {
    Open { seq: u64, path: String, clsid: GUID, size: (i32, i32), colors: Option<(u32, u32)>, reply: Sender<Result<(), String>> },
    Resize(i32, i32),
    Close,
}

/// The latest open request; a later open or a close makes earlier ones moot.
static SEQ: AtomicU64 = AtomicU64::new(0);
/// A handler is loaded, so the host window shows at PLACE.
static SHOWN: AtomicBool = AtomicBool::new(false);
/// Where the page last said the pane is; None while it's covered or gone.
static PLACE: Mutex<Option<Place>> = Mutex::new(None);
/// The host window (owned by the main window, made on the UI thread).
static HOST: AtomicIsize = AtomicIsize::new(0);
/// The main window, which PLACE is relative to.
static TOP: AtomicIsize = AtomicIsize::new(0);
/// What the host window paints behind the handler, as a COLORREF.
static BG: AtomicU32 = AtomicU32::new(0x00FF_FFFF);

/// The thread that talks to handlers. It's an STA of its own, so a handler
/// that takes seconds to load never stalls the window.
fn worker() -> &'static Mutex<Sender<Req>> {
    static TX: OnceLock<Mutex<Sender<Req>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = channel();
        std::thread::spawn(move || run(rx));
        Mutex::new(tx)
    })
}

fn send(req: Req) {
    let _ = worker().lock().unwrap().send(req);
}

// ponytail: a handler that never returns from DoPreview holds this thread and
// every later preview; starting a fresh thread after a timeout fixes that if it happens.
fn run(rx: Receiver<Req>) {
    crate::com::sta();
    let mut current: Option<IPreviewHandler> = None;
    let unload = |h: &mut Option<IPreviewHandler>| {
        if let Some(h) = h.take() {
            unsafe {
                let _ = h.Unload();
            }
        }
    };
    loop {
        let req = match rx.recv_timeout(Duration::from_millis(30)) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => {
                // Answer anything a handler sends back while idle.
                unsafe {
                    let mut msg = MSG::default();
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match req {
            Req::Open { seq, path, clsid, size, colors, reply } => {
                unload(&mut current);
                if SEQ.load(Ordering::SeqCst) != seq {
                    let _ = reply.send(Err("superseded".into()));
                    continue;
                }
                let host = HWND(HOST.load(Ordering::SeqCst) as *mut _);
                match unsafe { load(&path, &clsid, host, size, colors) } {
                    Ok(h) if SEQ.load(Ordering::SeqCst) == seq => {
                        current = Some(h);
                        let _ = reply.send(Ok(()));
                    }
                    Ok(h) => {
                        current = Some(h);
                        unload(&mut current);
                        let _ = reply.send(Err("superseded".into()));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Req::Resize(w, h) => {
                if let Some(p) = &current {
                    unsafe {
                        let _ = p.SetRect(&RECT { left: 0, top: 0, right: w, bottom: h });
                    }
                }
            }
            Req::Close => unload(&mut current),
        }
    }
}

/// Create the handler out of process, give it the file, and draw it into the host.
unsafe fn load(path: &str, clsid: &GUID, host: HWND, size: (i32, i32), colors: Option<(u32, u32)>) -> Result<IPreviewHandler, String> {
    let e = |e: windows::core::Error| e.message();
    unsafe {
        // Out of process only: a handler that crashes takes prevhost.exe with it, not Filezy.
        let unk: windows::core::IUnknown = CoCreateInstance(clsid, None, CLSCTX_LOCAL_SERVER).map_err(e)?;
        // Some handlers don't take "\\?\" paths; give them the plain one when it fits.
        let file = HSTRING::from(if path.len() < 260 { path.to_string() } else { paths::to_native(path) });
        if let Ok(i) = unk.cast::<IInitializeWithFile>() {
            i.Initialize(&file, STGM_READ.0).map_err(e)?;
        } else if let Ok(i) = unk.cast::<IInitializeWithItem>() {
            let item: IShellItem = SHCreateItemFromParsingName(&file, None).map_err(e)?;
            i.Initialize(&item, STGM_READ.0).map_err(e)?;
        } else if let Ok(i) = unk.cast::<IInitializeWithStream>() {
            let stream = SHCreateStreamOnFileEx(&file, (STGM_READ | STGM_SHARE_DENY_NONE).0, 0, false, None).map_err(e)?;
            i.Initialize(&stream, STGM_READ.0).map_err(e)?;
        } else {
            return Err("The previewer can't open files.".into());
        }
        let handler: IPreviewHandler = unk.cast().map_err(e)?;
        if let (Some((bg, fg)), Ok(v)) = (colors, unk.cast::<IPreviewHandlerVisuals>()) {
            let _ = v.SetBackgroundColor(COLORREF(bg));
            let _ = v.SetTextColor(COLORREF(fg));
        }
        handler.SetWindow(host, &RECT { left: 0, top: 0, right: size.0, bottom: size.1 }).map_err(e)?;
        handler.DoPreview().map_err(e)?;
        Ok(handler)
    }
}

/// Make the host window once. UI thread only.
fn host(top: HWND) -> HWND {
    let h = HOST.load(Ordering::SeqCst);
    if h != 0 {
        return HWND(h as *mut _);
    }
    unsafe extern "system" fn proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        // A click in the preview leaves Filezy's window the active one.
        if m == WM_MOUSEACTIVATE {
            return LRESULT(MA_NOACTIVATE as isize);
        }
        if m == WM_ERASEBKGND {
            unsafe {
                let mut r = RECT::default();
                let _ = GetClientRect(h, &mut r);
                let brush = CreateSolidBrush(COLORREF(BG.load(Ordering::SeqCst)));
                FillRect(HDC(wp.0 as *mut _), &r, brush);
                let _ = DeleteObject(brush.into());
            }
            return LRESULT(1);
        }
        unsafe { DefWindowProcW(h, m, wp, lp) }
    }
    unsafe {
        let inst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: inst.into(),
            lpszClassName: w!("FilezyPreviewHost"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&class);
        // Owned by the main window: always above it, minimized and closed with it.
        let ex = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
        let h = CreateWindowExW(
            ex,
            w!("FilezyPreviewHost"),
            w!(""),
            WS_POPUP | WS_CLIPCHILDREN,
            0,
            0,
            0,
            0,
            Some(top),
            None,
            Some(inst.into()),
            None,
        )
        .unwrap_or_default();
        HOST.store(h.0 as isize, Ordering::SeqCst);
        TOP.store(top.0 as isize, Ordering::SeqCst);
        h
    }
}

/// Show the host over PLACE when a handler is loaded and the pane is
/// uncovered; else hide it. UI thread only. Also called when the main window
/// moves, since an owned window doesn't move with it.
pub fn apply() {
    let h = HOST.load(Ordering::SeqCst);
    if h == 0 {
        return;
    }
    let h = HWND(h as *mut _);
    let top = HWND(TOP.load(Ordering::SeqCst) as *mut _);
    let place = *PLACE.lock().unwrap();
    unsafe {
        match place.filter(|_| SHOWN.load(Ordering::SeqCst)) {
            Some(p) => {
                let mut at = POINT { x: p.x, y: p.y };
                let _ = ClientToScreen(top, &mut at);
                // Directly above the main window, never above another app's window over it.
                let above = GetWindow(top, GW_HWNDPREV).ok();
                let (after, flags) = match above {
                    Some(a) if a == h => (None, SWP_NOZORDER),
                    Some(a) => (Some(a), Default::default()),
                    None => (Some(HWND_TOP), Default::default()),
                };
                let _ = SetWindowPos(h, after, at.x, at.y, p.w, p.h, flags | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            }
            None => {
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
    }
}

fn refresh(app: &AppHandle) {
    let _ = app.run_on_main_thread(apply);
}

/// Preview `path` with its registered handler at `place`. False when a later
/// request replaced this one; an error when the handler couldn't show it.
pub async fn open(app: AppHandle, path: String, place: Place, colors: Option<(u32, u32)>) -> Result<bool, String> {
    let seq = SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    SHOWN.store(false, Ordering::SeqCst);
    *PLACE.lock().unwrap() = Some(place);
    refresh(&app);
    // Decided here, never taken from the page: which class runs, and whether
    // the file may be handed to one at all.
    let file = path.clone();
    let checked = tauri::async_runtime::spawn_blocking(move || {
        let clsid = handler(&ext_of(&file)).ok_or_else(|| "No preview available.".to_string())?;
        if zone(&paths::to_native(&file)) >= 3 {
            return Err("The file came from the internet.".to_string());
        }
        Ok(clsid)
    })
    .await
    .map_err(|e| e.to_string())?;
    let clsid = checked?;
    if let Some((bg, _)) = colors {
        BG.store(bg, Ordering::SeqCst);
    }
    let top = crate::owner(&app);
    let made = crate::on_main(&app, move || {
        let h = host(HWND(top as *mut _));
        // Sized while still hidden: some handlers size themselves to it rather than to SetWindow's rectangle.
        unsafe {
            let _ = SetWindowPos(h, None, 0, 0, place.w, place.h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        h.0 as isize
    });
    if made.await? == 0 {
        return Err("The preview window couldn't be made.".into());
    }
    let (reply, done) = channel();
    send(Req::Open { seq, path, clsid, size: (place.w, place.h), colors, reply });
    let result = tauri::async_runtime::spawn_blocking(move || done.recv().unwrap_or_else(|_| Err("stopped".into())))
        .await
        .map_err(|e| e.to_string())?;
    match result {
        Ok(()) if SEQ.load(Ordering::SeqCst) == seq => {
            SHOWN.store(true, Ordering::SeqCst);
            refresh(&app);
            // As Explorer does once the window shows; Word lays itself out only then.
            if let Some(p) = *PLACE.lock().unwrap() {
                send(Req::Resize(p.w, p.h));
            }
            Ok(true)
        }
        Ok(()) => Ok(false),
        Err(e) if e == "superseded" => Ok(false),
        Err(e) => Err(e),
    }
}

/// The pane moved or resized (Some), or a menu or dialog now covers it (None).
pub fn place(app: &AppHandle, place: Option<Place>) {
    *PLACE.lock().unwrap() = place;
    refresh(app);
    if let Some(p) = place {
        if SHOWN.load(Ordering::SeqCst) {
            send(Req::Resize(p.w, p.h));
        }
    }
}

/// Answer a `preview` request, which names a file the way the page's
/// convertFileSrc(path, "preview") does: percent-encoded after the host. Only
/// the file `info` chose last is served, in the ranges a media element asks for.
pub fn serve(request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    use tauri::http::{header, Response, StatusCode};
    let status = |code: StatusCode| Response::builder().status(code).body(Vec::new()).unwrap();
    let Some(path) = percent_decode(request.uri().path().trim_start_matches('/')) else { return status(StatusCode::BAD_REQUEST) };
    let Ok(canon) = std::fs::canonicalize(paths::to_native(&path)) else { return status(StatusCode::NOT_FOUND) };
    if SERVED.lock().unwrap().as_ref() != Some(&canon) {
        return status(StatusCode::FORBIDDEN);
    }
    let Ok(mut file) = std::fs::File::open(&canon) else { return status(StatusCode::NOT_FOUND) };
    let Ok(len) = file.metadata().map(|m| m.len()) else { return status(StatusCode::NOT_FOUND) };
    let range = request.headers().get(header::RANGE).and_then(|v| v.to_str().ok()).map(|v| byte_range(v, len));
    let (start, end) = match range {
        None => (0, len),
        Some(Some(r)) => r,
        Some(None) => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Vec::new())
                .unwrap()
        }
    };
    let mut body = Vec::with_capacity((end - start) as usize);
    if file.seek(SeekFrom::Start(start)).and_then(|_| file.take(end - start).read_to_end(&mut body)).is_err() {
        return status(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let mut response = Response::builder()
        .header(header::CONTENT_TYPE, mime(&ext_of(&path)))
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    if range.is_some() && !body.is_empty() {
        let last = start + body.len() as u64 - 1;
        response = response.status(StatusCode::PARTIAL_CONTENT).header(header::CONTENT_RANGE, format!("bytes {start}-{last}/{len}"));
    }
    response.body(body).unwrap()
}

/// Most a ranged response carries; the player asks for the rest as it needs it.
const RANGE_MAX: u64 = 4 << 20;

/// The first range of a Range header as [start, end) within `len`, at most
/// RANGE_MAX long; None when it can't be satisfied.
fn byte_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let first = header.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = first.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", n) => (len.saturating_sub(n.parse().ok()?), len),
        (s, "") => (s.parse().ok()?, len),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.saturating_add(1).min(len)),
    };
    (start < end).then(|| (start, end.min(start + RANGE_MAX)))
}

fn percent_decode(s: &str) -> Option<String> {
    let mut out = Vec::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        out.push(if b == b'%' {
            let hex = [bytes.next()?, bytes.next()?];
            u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?
        } else {
            b
        });
    }
    String::from_utf8(out).ok()
}

fn mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "weba" => "audio/webm",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mov" => "video/quicktime",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// Unload whatever handler is showing.
pub fn close(app: &AppHandle) {
    SEQ.fetch_add(1, Ordering::SeqCst);
    SHOWN.store(false, Ordering::SeqCst);
    refresh(app);
    send(Req::Close);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_text_as_notepad_does() {
        assert_eq!(decode(b"\xEF\xBB\xBFcaf\xC3\xA9", false).unwrap(), "café");
        assert_eq!(decode(b"\xFF\xFEh\0i\0", false).unwrap(), "hi");
        assert_eq!(decode(b"\xFE\xFF\0h\0i", false).unwrap(), "hi");
        // UTF-16 without a byte-order mark.
        assert_eq!(decode(b"h\0e\0l\0l\0o\0", false).unwrap(), "hello");
        assert_eq!(decode("plain ünïcode".as_bytes(), false).unwrap(), "plain ünïcode");
        // A character cut in two by the size limit is dropped, not taken for binary.
        assert_eq!(decode(b"ab\xC3", true).unwrap(), "ab");
        // Not UTF-8: the ANSI code page (é is 0xE9 in Western code pages).
        assert!(decode(b"caf\xE9 au lait", false).unwrap().starts_with("caf"));
        assert!(decode(b"MZ\x90\0\x03\0\0\0", false).is_none());
        assert!(decode(&[1, 2, 3, 4, 5, 6, 7, 8, b'a'], false).is_none());
    }

    #[test]
    fn preview_requests() {
        assert_eq!(byte_range("bytes=0-", 10), Some((0, 10)));
        assert_eq!(byte_range("bytes=2-4", 10), Some((2, 5)));
        assert_eq!(byte_range("bytes=-3", 10), Some((7, 10)));
        assert_eq!(byte_range("bytes=5-100", 10), Some((5, 10)));
        assert_eq!(byte_range("bytes=0-", 100 << 20), Some((0, RANGE_MAX)));
        assert_eq!(byte_range("bytes=10-", 10), None);
        assert_eq!(byte_range("items=0-1", 10), None);
        assert_eq!(percent_decode("C%3A%5CMy%20Files%5C%C3%BC.png").as_deref(), Some(r"C:\My Files\ü.png"));
        assert_eq!(percent_decode("bad%2"), None);
    }

    #[test]
    fn serves_only_the_file_shown_last() {
        use tauri::http::{Request, StatusCode};
        let dir = std::env::temp_dir().join(format!("filezy-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (shown, other) = (dir.join("shown.png"), dir.join("other.png"));
        std::fs::write(&shown, b"0123456789").unwrap();
        std::fs::write(&other, b"secret").unwrap();
        let url = |p: &std::path::Path| {
            let enc: String = p.to_string_lossy().bytes().map(|b| format!("%{b:02X}")).collect();
            format!("http://preview.localhost/{enc}")
        };
        let get = |p: &std::path::Path, range: Option<&str>| {
            let mut r = Request::builder().uri(url(p));
            if let Some(range) = range {
                r = r.header("range", range);
            }
            serve(&r.body(Vec::new()).unwrap())
        };
        assert_eq!(info(&shown.to_string_lossy(), false).unwrap().kind, "image");
        let full = get(&shown, None);
        assert_eq!((full.status(), full.body().as_slice()), (StatusCode::OK, &b"0123456789"[..]));
        assert_eq!(full.headers()["content-type"], "image/png");
        let part = get(&shown, Some("bytes=2-4"));
        assert_eq!((part.status(), part.body().as_slice()), (StatusCode::PARTIAL_CONTENT, &b"234"[..]));
        assert_eq!(part.headers()["content-range"], "bytes 2-4/10");
        assert_eq!(get(&shown, Some("bytes=20-")).status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(get(&other, None).status(), StatusCode::FORBIDDEN);
        // Showing something else withdraws the first file.
        assert_eq!(info(&other.to_string_lossy(), false).unwrap().kind, "image");
        assert_eq!(get(&shown, None).status(), StatusCode::FORBIDDEN);
        assert_eq!(get(&other, None).status(), StatusCode::OK);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn handlers_come_from_the_registry() {
        // Windows' own plain-text previewer is never used; the page shows text.
        assert!(handler("txt").is_none());
        assert!(handler("").is_none());
        assert!(handler("no-such-extension-filezy").is_none());
    }
}
