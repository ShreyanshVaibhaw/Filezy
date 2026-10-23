use base64::Engine;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{DeleteObject, GetDC, GetDIBits, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON, SHGFI_TYPENAME, SHGFI_USEFILEATTRIBUTES};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

use crate::fs_list::wide;

#[derive(Clone, Serialize)]
pub struct ExtMeta {
    pub icon: Option<String>,
    pub type_name: String,
}

static EXT_CACHE: Mutex<Option<HashMap<String, ExtMeta>>> = Mutex::new(None);

/// Icon + friendly type name for an extension, resolved without touching disk.
pub fn ext_meta(ext: &str, is_dir: bool) -> ExtMeta {
    let key = if is_dir { "//dir".to_string() } else { ext.to_lowercase() };
    if let Some(cache) = EXT_CACHE.lock().unwrap().as_ref() {
        if let Some(m) = cache.get(&key) {
            return m.clone();
        }
    }
    let fake = if is_dir { "folder".to_string() } else { format!("f.{ext}") };
    let attrs = if is_dir { FILE_ATTRIBUTE_DIRECTORY } else { FILE_ATTRIBUTE_NORMAL };
    let meta = shell_info(&fake, attrs.0, true);
    EXT_CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, meta.clone());
    meta
}

/// Real per-file icon (exe, lnk, ico, custom folders). Not cached here; the frontend caches by path.
pub fn path_icon(path: &str) -> Option<String> {
    shell_info(path, 0, false).icon
}

/// Explorer-quality thumbnail/large icon via IShellItemImageFactory (images get real
/// picture thumbnails, everything else gets its high-res icon). `px` is in device
/// pixels, which is what the factory works in.
pub fn thumbnail(path: &str, px: i32) -> Option<String> {
    image_of(path, px.clamp(16, 1024), false)
}

/// Force the ICON (never a thumbnail): special-folder icons (Desktop, Documents,
/// OneDrive overlays) and drive icons for the app chrome.
pub fn shell_icon_png(path: &str, size: i32) -> Option<String> {
    image_of(path, size, true)
}

fn image_of(path: &str, size: i32, icon_only: bool) -> Option<String> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY};
    crate::com::sta();
    unsafe {
        let wpath = wide(path);
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(wpath.as_ptr()), None).ok()?;
        let flags = if icon_only { SIIGBF(SIIGBF_BIGGERSIZEOK.0 | SIIGBF_ICONONLY.0) } else { SIIGBF_BIGGERSIZEOK };
        let hbm = factory.GetImage(SIZE { cx: size, cy: size }, flags).ok()?;

        let hdc = GetDC(None);
        let mut header = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, ..Default::default() };
        let mut probe = BITMAPINFO { bmiHeader: header, ..Default::default() };
        GetDIBits(hdc, hbm, 0, 0, None, &mut probe, DIB_RGB_COLORS);
        let w = probe.bmiHeader.biWidth;
        let h = probe.bmiHeader.biHeight.abs();
        if w <= 0 || h <= 0 || w > 1024 || h > 1024 {
            let _ = DeleteObject(hbm.into());
            ReleaseDC(None, hdc);
            return None;
        }
        header.biWidth = w;
        header.biHeight = -h;
        header.biPlanes = 1;
        header.biBitCount = 32;
        header.biCompression = BI_RGB.0;
        let mut bmi = BITMAPINFO { bmiHeader: header, ..Default::default() };
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        let got = GetDIBits(hdc, hbm, 0, h as u32, Some(pixels.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
        let _ = DeleteObject(hbm.into());
        ReleaseDC(None, hdc);
        if got == 0 {
            return None;
        }
        let opaque = pixels.chunks_exact(4).all(|p| p[3] == 0);
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
            if opaque {
                px[3] = 255;
            }
        }
        encode_png(&pixels, w as u32, h as u32)
    }
}

fn encode_png(rgba: &[u8], w: u32, h: u32) -> Option<String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(out)))
}

fn shell_info(path: &str, attrs: u32, by_attr: bool) -> ExtMeta {
    crate::com::sta();
    let wpath = wide(path);
    let mut sfi = SHFILEINFOW::default();
    let mut flags = SHGFI_ICON | SHGFI_SMALLICON | SHGFI_TYPENAME;
    if by_attr {
        flags |= SHGFI_USEFILEATTRIBUTES;
    }
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wpath.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(attrs),
            Some(&mut sfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };
    if ok == 0 {
        return ExtMeta { icon: None, type_name: String::new() };
    }
    let len = sfi.szTypeName.iter().position(|&c| c == 0).unwrap_or(80);
    let type_name = String::from_utf16_lossy(&sfi.szTypeName[..len]);
    let icon = if sfi.hIcon.is_invalid() { None } else { hicon_to_data_url(sfi.hIcon) };
    if !sfi.hIcon.is_invalid() {
        unsafe { DestroyIcon(sfi.hIcon).ok() };
    }
    ExtMeta { icon, type_name }
}

fn hicon_to_data_url(hicon: HICON) -> Option<String> {
    unsafe {
        let mut info = ICONINFO::default();
        GetIconInfo(hicon, &mut info).ok()?;
        let hdc = GetDC(None);

        let mut header = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, ..Default::default() };
        let mut probe = BITMAPINFO { bmiHeader: header, ..Default::default() };
        // First call with no buffer fills in dimensions.
        GetDIBits(hdc, info.hbmColor, 0, 0, None, &mut probe, DIB_RGB_COLORS);
        let w = probe.bmiHeader.biWidth;
        let h = probe.bmiHeader.biHeight.abs();
        if w <= 0 || h <= 0 || w > 512 || h > 512 {
            let _ = DeleteObject(info.hbmColor.into());
            let _ = DeleteObject(info.hbmMask.into());
            ReleaseDC(None, hdc);
            return None;
        }

        header.biWidth = w;
        header.biHeight = -h; // top-down
        header.biPlanes = 1;
        header.biBitCount = 32;
        header.biCompression = BI_RGB.0;
        let mut bmi = BITMAPINFO { bmiHeader: header, ..Default::default() };
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        let got = GetDIBits(hdc, info.hbmColor, 0, h as u32, Some(pixels.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);

        let mut mask_pixels = None;
        if got != 0 && pixels.chunks_exact(4).all(|p| p[3] == 0) {
            // No alpha channel: derive it from the AND mask.
            let mut mp = vec![0u8; (w * h * 4) as usize];
            let mut mbmi = BITMAPINFO { bmiHeader: header, ..Default::default() };
            if GetDIBits(hdc, info.hbmMask, 0, h as u32, Some(mp.as_mut_ptr() as *mut _), &mut mbmi, DIB_RGB_COLORS) != 0 {
                mask_pixels = Some(mp);
            }
        }

        let _ = DeleteObject(info.hbmColor.into());
        let _ = DeleteObject(info.hbmMask.into());
        ReleaseDC(None, hdc);
        if got == 0 {
            return None;
        }

        // BGRA -> RGBA, applying mask-derived alpha when needed.
        for (i, px) in pixels.chunks_exact_mut(4).enumerate() {
            px.swap(0, 2);
            if let Some(m) = &mask_pixels {
                px[3] = if m[i * 4] == 0 { 255 } else { 0 };
            }
        }

        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().ok()?;
            writer.write_image_data(&pixels).ok()?;
        }
        Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(out)))
    }
}
