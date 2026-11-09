//! Disposition du clavier : celle de la fenêtre active (comme l'indicateur « FRA » de Windows),
//! et la liste des dispositions installées pour en changer.

use std::collections::HashMap;
use std::ffi::c_void;
use std::time::Duration;

use serde::Serialize;
use tauri::AppHandle;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Globalization::{
    GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SABBREVLANGNAME, LOCALE_SLOCALIZEDDISPLAYNAME,
};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList, HKL};
use windows::Win32::UI::Shell::SHLoadIndirectString;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, WM_INPUTLANGCHANGEREQUEST,
};

use crate::{appbar, publish, sys, tasks};

#[derive(Serialize, Clone, PartialEq)]
struct Layout {
    id: isize,
    /// Code court affiché dans la barre (ex. « FRA »).
    code: String,
    /// Langue (ex. « Français (France) »).
    name: String,
    /// Disposition (ex. « Français », « États-Unis »).
    detail: String,
}

#[derive(Serialize, PartialEq)]
struct Keyboard {
    current: isize,
    layouts: Vec<Layout>,
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut known: HashMap<isize, Layout> = HashMap::new();
        loop {
            std::thread::sleep(Duration::from_millis(if appbar::fullscreen_active() { 3000 } else { 400 }));
            let layouts: Vec<Layout> =
                installed().into_iter().map(|id| known.entry(id).or_insert_with(|| describe(id)).clone()).collect();
            publish(&app, "keyboard", &Keyboard { current: current(), layouts });
        }
    });
}

/// Fenêtre dont on suit la disposition : celle au premier plan, ou la dernière active si c'est la barre.
fn target() -> HWND {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() || fg == appbar::hwnd() {
        tasks::active_window()
    } else {
        fg
    }
}

fn current() -> isize {
    unsafe {
        let thread = GetWindowThreadProcessId(target(), None);
        GetKeyboardLayout(thread).0 as isize
    }
}

fn installed() -> Vec<isize> {
    unsafe {
        let n = GetKeyboardLayoutList(None).max(0) as usize;
        let mut list = vec![HKL::default(); n];
        let n = GetKeyboardLayoutList(Some(&mut list)).max(0) as usize;
        list[..n].iter().map(|h| h.0 as isize).collect()
    }
}

pub fn select(id: isize) {
    unsafe {
        let _ = PostMessageW(Some(target()), WM_INPUTLANGCHANGEREQUEST, WPARAM(0), LPARAM(id));
    }
}

fn describe(id: isize) -> Layout {
    let lang = (id as usize & 0xFFFF) as u32;
    let mut locale = [0u16; 85];
    let n = unsafe { LCIDToLocaleName(lang, Some(&mut locale), 0) };
    let locale = PCWSTR(locale.as_ptr());
    let info = |kind| {
        let mut buf = [0u16; 128];
        let n = unsafe { GetLocaleInfoEx(locale, kind, Some(&mut buf)) };
        String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize])
    };
    let (code, name) =
        if n > 0 { (info(LOCALE_SABBREVLANGNAME), info(LOCALE_SLOCALIZEDDISPLAYNAME)) } else { Default::default() };
    Layout {
        id,
        code: if code.is_empty() { "?".into() } else { code },
        name,
        detail: layout_name(id).unwrap_or_default(),
    }
}

/// Nom localisé de la disposition, lu dans le registre (clé « Keyboard Layouts\<KLID> »).
fn layout_name(id: isize) -> Option<String> {
    let device = (id as usize >> 16) & 0xFFFF;
    let klid = match device & 0xF000 {
        0xF000 => return None, // disposition « spéciale » (Dvorak...) : identifiant indirect, on s'abstient
        0xE000 => format!("{:08X}", id as usize as u32), // IME
        _ => format!("{device:08X}"),
    };
    let key = sys::wide(&format!("SYSTEM\\CurrentControlSet\\Control\\Keyboard Layouts\\{klid}"));
    let mut buf = [0u16; 260];
    let mut size = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            windows::core::w!("Layout Display Name"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut size),
        )
        .ok()
        .ok()?;
        // Valeur du type « @%SystemRoot%\system32\input.dll,-5011 » : chaîne localisée d'une DLL.
        let mut out = [0u16; 128];
        SHLoadIndirectString(PCWSTR(buf.as_ptr()), &mut out, None).ok()?;
        let len = out.iter().position(|&c| c == 0).unwrap_or(out.len());
        Some(String::from_utf16_lossy(&out[..len]))
    }
}
