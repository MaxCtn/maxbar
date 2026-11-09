//! Extraction d'icônes Windows -> PNG en data URL.

use std::ffi::c_void;

use base64::{engine::general_purpose::STANDARD, Engine};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::Shell::{SHDefExtractIconW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetClassLongPtrW, GetIconInfo, SendMessageTimeoutW, GCLP_HICON, HICON, ICONINFO, ICON_BIG,
    SMTO_ABORTIFHUNG, SMTO_BLOCK, WM_GETICON,
};

use crate::sys::wide;

/// Taille demandée : assez grande pour rester nette à 150-200 % de mise à l'échelle.
const SIZE: u32 = 48;

pub fn from_path(path: &str) -> Option<String> {
    let p = wide(path);
    unsafe {
        let mut icon = HICON::default();
        if SHDefExtractIconW(PCWSTR(p.as_ptr()), 0, 0, Some(&mut icon), None, SIZE).is_ok() && !icon.is_invalid() {
            let out = encode(icon);
            let _ = DestroyIcon(icon);
            if out.is_some() {
                return out;
            }
        }
        // Raccourcis, dossiers, types sans ressource d'icône.
        let mut info = SHFILEINFOW::default();
        let ok = SHGetFileInfoW(
            PCWSTR(p.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if ok != 0 && !info.hIcon.is_invalid() {
            let out = encode(info.hIcon);
            let _ = DestroyIcon(info.hIcon);
            return out;
        }
    }
    None
}

/// Icône propre à la fenêtre (ne pas détruire : elle appartient à la fenêtre).
pub fn from_window(h: HWND) -> Option<String> {
    unsafe {
        let mut res = 0usize;
        let _ = SendMessageTimeoutW(
            h,
            WM_GETICON,
            WPARAM(ICON_BIG as usize),
            LPARAM(0),
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            60,
            Some(&mut res),
        );
        if res == 0 {
            res = GetClassLongPtrW(h, GCLP_HICON);
        }
        (res != 0).then(|| encode(HICON(res as *mut c_void))).flatten()
    }
}

unsafe fn bits(bmp: HBITMAP, w: i32, h: i32) -> Option<Vec<u8>> {
    let mut bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut px = vec![0u8; (w * h * 4) as usize];
    let dc = GetDC(None);
    let lines = GetDIBits(dc, bmp, 0, h as u32, Some(px.as_mut_ptr() as *mut c_void), &mut bi, DIB_RGB_COLORS);
    ReleaseDC(None, dc);
    (lines > 0).then_some(px)
}

/// Icône appartenant à un autre process (zone de notification) : lue sans être détruite.
pub fn encode_icon(icon: HICON) -> Option<String> {
    unsafe { encode(icon) }
}

unsafe fn encode(icon: HICON) -> Option<String> {
    let mut ii = ICONINFO::default();
    GetIconInfo(icon, &mut ii).ok()?;
    let color = ii.hbmColor;
    let mask = ii.hbmMask;
    let cleanup = || {
        if !color.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(color.0));
        }
        if !mask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(mask.0));
        }
    };
    if color.is_invalid() {
        cleanup();
        return None;
    }
    let mut bm = BITMAP::default();
    GetObjectW(HGDIOBJ(color.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void));
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    if w <= 0 || h <= 0 || w > 256 || h > 256 {
        cleanup();
        return None;
    }
    let Some(mut px) = bits(color, w, h) else {
        cleanup();
        return None;
    };
    // Icônes anciennes sans canal alpha : on reconstruit l'alpha depuis le masque.
    if !px.chunks_exact(4).any(|p| p[3] != 0) {
        if let Some(m) = (!mask.is_invalid()).then(|| bits(mask, w, h)).flatten() {
            for (p, mk) in px.chunks_exact_mut(4).zip(m.chunks_exact(4)) {
                p[3] = if mk[0] == 0 { 255 } else { 0 };
            }
        } else {
            px.chunks_exact_mut(4).for_each(|p| p[3] = 255);
        }
    }
    cleanup();
    for p in px.chunks_exact_mut(4) {
        p.swap(0, 2); // BGRA -> RGBA
    }

    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().ok()?;
        wr.write_image_data(&px).ok()?;
    }
    Some(format!("data:image/png;base64,{}", STANDARD.encode(out)))
}

/// Bitmap 32 bits (ex. `IShellItemImageFactory::GetImage`) -> PNG. Le bitmap reste à libérer par l'appelant.
pub fn from_bitmap(bmp: HBITMAP) -> Option<String> {
    unsafe {
        let mut bm = BITMAP::default();
        let size = std::mem::size_of::<BITMAP>() as i32;
        if GetObjectW(HGDIOBJ(bmp.0), size, Some(&mut bm as *mut _ as *mut c_void)) == 0 {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        if w <= 0 || h <= 0 || w > 256 || h > 256 {
            return None;
        }
        let mut px = bits(bmp, w, h)?;
        if px.chunks_exact(4).all(|p| p[3] == 0) {
            px.chunks_exact_mut(4).for_each(|p| p[3] = 255);
        } else if px.chunks_exact(4).all(|p| p[0].max(p[1]).max(p[2]) <= p[3]) {
            // Alpha prémultiplié (aucune composante ne dépasse l'alpha) : PNG attend un alpha droit.
            for p in px.chunks_exact_mut(4) {
                let a = u32::from(p[3]);
                if a > 0 && a < 255 {
                    for c in &mut p[..3] {
                        *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
                    }
                }
            }
        }
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut wr = enc.write_header().ok()?;
            wr.write_image_data(&px).ok()?;
        }
        Some(format!("data:image/png;base64,{}", STANDARD.encode(out)))
    }
}
