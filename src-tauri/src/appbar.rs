//! Geometrie de la barre.
//!
//! - Une bande de l'ecran est reservee via l'API "AppBar" : les fenetres maximisees s'arretent au bord.
//! - La fenetre couvre tout l'ecran (moins 1 px, pour ne pas etre prise pour une app plein ecran),
//!   mais une *region* Win32 limite la zone visible et cliquable a la barre + aux elements ouverts
//!   (ilot, panneaux, volet). Le reste est invisible et laisse passer les clics.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, GetMonitorInfoW, MonitorFromWindow, SetWindowRgn,
    HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST, RGN_OR,
};
use windows::Win32::UI::Shell::{
    DefSubclassProc, SHAppBarMessage, SetWindowSubclass, ABE_BOTTOM, ABE_TOP, ABM_GETSTATE, ABM_NEW, ABM_QUERYPOS,
    ABM_REMOVE, ABM_SETPOS, ABM_SETSTATE, ABN_POSCHANGED, ABS_ALWAYSONTOP, ABS_AUTOHIDE, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, IsWindowVisible, RegisterWindowMessageW,
    SetWindowLongPtrW, SetWindowPos, ShowWindowAsync, GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE, SW_HIDE,
    SW_SHOWNOACTIVATE, WM_APP, WS_EX_TOOLWINDOW,
};

use crate::{publish, sys::class_name, tray};

const CALLBACK_MSG: u32 = WM_APP + 0x51;

static APP: OnceLock<AppHandle> = OnceLock::new();
static BAR: AtomicIsize = AtomicIsize::new(0);
static REGISTERED: AtomicBool = AtomicBool::new(false);
static HIDDEN: AtomicBool = AtomicBool::new(false);
static BOTTOM: AtomicBool = AtomicBool::new(false);
static HEIGHT: AtomicU32 = AtomicU32::new(42);
static SCALE: AtomicU64 = AtomicU64::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static EXPLORER: AtomicIsize = AtomicIsize::new(0);
const ZERO: RECT = RECT { left: 0, top: 0, right: 0, bottom: 0 };
/// Bande reservee et rectangle de la fenetre, en pixels physiques ecran.
static BAND: Mutex<RECT> = Mutex::new(ZERO);
static WIN: Mutex<RECT> = Mutex::new(ZERO);
static EXTRA: Mutex<Vec<Shape>> = Mutex::new(Vec::new());

/// Rectangle arrondi en px CSS, relatif a la fenetre (envoye par l'interface).
#[derive(Deserialize, Clone, Copy, PartialEq)]
pub struct Shape {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
}

/// Position de la barre dans la fenetre, en px CSS.
#[derive(Serialize, PartialEq)]
struct Geometry {
    bar_top: f64,
    bar_height: f64,
    bottom: bool,
}

pub fn hwnd() -> HWND {
    HWND(BAR.load(Relaxed) as *mut c_void)
}

fn scale() -> f64 {
    f64::from_bits(SCALE.load(Relaxed)).max(1.0)
}

fn data(h: HWND) -> APPBARDATA {
    APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: h, ..Default::default() }
}

pub fn init(app: &AppHandle, bottom: bool, height: u32) -> tauri::Result<()> {
    let _ = APP.set(app.clone());
    let win = app.get_webview_window("bar").ok_or(tauri::Error::WindowNotFound)?;
    let h = HWND(win.hwnd()?.0 as *mut c_void);
    BAR.store(h.0 as isize, Relaxed);
    EXPLORER.store(tray::explorer_tray().0 as isize, Relaxed);
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        SetWindowLongPtrW(h, GWL_EXSTYLE, ex | WS_EX_TOOLWINDOW.0 as isize);
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Relaxed);
        let _ = SetWindowSubclass(h, Some(subclass), 0x6d62, 0);
    }
    configure(bottom, height);
    Ok(())
}

pub fn configure(bottom: bool, height: u32) {
    BOTTOM.store(bottom, Relaxed);
    HEIGHT.store(height, Relaxed);
    reposition();
}

fn reposition() {
    let Some(app) = APP.get() else { return };
    let Ok(Some(mon)) = app.primary_monitor() else { return };
    let scale = mon.scale_factor();
    SCALE.store(scale.to_bits(), Relaxed);
    let (p, s) = (mon.position(), mon.size());
    let full = RECT { left: p.x, top: p.y, right: p.x + s.width as i32, bottom: p.y + s.height as i32 };
    let thick = (HEIGHT.load(Relaxed) as f64 * scale).round() as i32;
    let bottom = BOTTOM.load(Relaxed);
    let clamp = |rc: &mut RECT| {
        if bottom {
            rc.top = rc.bottom - thick
        } else {
            rc.bottom = rc.top + thick
        }
    };
    let mut d = data(hwnd());
    d.uCallbackMessage = CALLBACK_MSG;
    d.uEdge = if bottom { ABE_BOTTOM } else { ABE_TOP };
    d.rc = full;
    clamp(&mut d.rc);
    unsafe {
        if !REGISTERED.swap(true, Relaxed) {
            SHAppBarMessage(ABM_NEW, &mut d);
        }
        SHAppBarMessage(ABM_QUERYPOS, &mut d);
        clamp(&mut d.rc);
        SHAppBarMessage(ABM_SETPOS, &mut d);
    }
    // 1 px de moins que l'ecran, du cote oppose a la barre.
    let win = if bottom { RECT { top: full.top + 1, ..full } } else { RECT { bottom: full.bottom - 1, ..full } };
    *BAND.lock().unwrap() = d.rc;
    *WIN.lock().unwrap() = win;
    place();
    apply_region();
    publish(
        app,
        "geometry",
        &Geometry {
            bar_top: (d.rc.top - win.top) as f64 / scale,
            bar_height: (d.rc.bottom - d.rc.top) as f64 / scale,
            bottom,
        },
    );
}

pub fn place() {
    let r = *WIN.lock().unwrap();
    unsafe {
        let _ = SetWindowPos(hwnd(), Some(HWND_TOPMOST), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOACTIVATE);
    }
}

/// Zones visibles en plus de la barre (ilot deploye, panneaux, volet lateral).
pub fn set_shapes(shapes: Vec<Shape>) {
    let mut extra = EXTRA.lock().unwrap();
    if *extra == shapes {
        return;
    }
    *extra = shapes;
    drop(extra);
    apply_region();
}

fn apply_region() {
    let (band, win) = (*BAND.lock().unwrap(), *WIN.lock().unwrap());
    let shapes = EXTRA.lock().unwrap().clone();
    let s = scale();
    let px = |v: f64| (v * s).round() as i32;
    unsafe {
        let region = CreateRectRgn(band.left - win.left, band.top - win.top, band.right - win.left, band.bottom - win.top);
        for sh in shapes.iter().filter(|sh| sh.w > 0.0 && sh.h > 0.0) {
            let (x, y) = (px(sh.x), px(sh.y));
            let d = px(sh.r) * 2;
            let part = CreateRoundRectRgn(x, y, x + px(sh.w) + 1, y + px(sh.h) + 1, d, d);
            CombineRgn(Some(region), Some(region), Some(part), RGN_OR);
            let _ = DeleteObject(HGDIOBJ(part.0));
        }
        // Le systeme devient proprietaire de la region.
        SetWindowRgn(hwnd(), Some(region), true);
    }
}

pub fn show() {
    unsafe {
        let _ = ShowWindowAsync(hwnd(), SW_SHOWNOACTIVATE);
    }
    place();
}

pub fn remove() {
    if TASKBAR_HIDDEN.load(Relaxed) {
        show_taskbars(true);
    }
    if REGISTERED.swap(false, Relaxed) {
        let mut d = data(hwnd());
        unsafe {
            SHAppBarMessage(ABM_REMOVE, &mut d);
        }
    }
}

unsafe extern "system" fn subclass(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if msg == CALLBACK_MSG && wp.0 as u32 == ABN_POSCHANGED {
        reposition();
    } else if msg != 0 && msg == TASKBAR_CREATED.load(Relaxed) {
        // Vrai redemarrage d'Explorer (pas notre propre diffusion) : il a oublie notre AppBar.
        let current = tray::explorer_tray().0 as isize;
        if EXPLORER.swap(current, Relaxed) != current {
            REGISTERED.store(false, Relaxed);
            reposition();
        }
    }
    DefSubclassProc(h, msg, wp, lp)
}

/// Vrai quand une app plein ecran (jeu, video) est au premier plan.
pub fn fullscreen_active() -> bool {
    HIDDEN.load(Relaxed)
}

/// Cache la barre quand l'app au premier plan est en plein ecran (jeu, video).
pub fn check_fullscreen() {
    let bar = hwnd();
    if bar.is_invalid() {
        return;
    }
    enforce_taskbar();
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == bar {
            return;
        }
        let full = is_fullscreen(fg, bar);
        if full != HIDDEN.swap(full, Relaxed) {
            let _ = ShowWindowAsync(bar, if full { SW_HIDE } else { SW_SHOWNOACTIVATE });
            set_webview_visible(!full);
        }
    }
}

/// Une WebView invisible ne dessine plus et ralentit ses timers : 0 % CPU pendant un jeu.
fn set_webview_visible(visible: bool) {
    let Some(win) = APP.get().and_then(|app| app.get_webview_window("bar")) else { return };
    let _ = win.with_webview(move |wv| unsafe {
        let _ = wv.controller().SetIsVisible(visible);
    });
}

unsafe fn is_fullscreen(fg: HWND, bar: HWND) -> bool {
    if matches!(class_name(fg).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
        return false;
    }
    let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
    if mon != MonitorFromWindow(bar, MONITOR_DEFAULTTONEAREST) {
        return false;
    }
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !GetMonitorInfoW(mon, &mut mi).as_bool() {
        return false;
    }
    let mut r = RECT::default();
    if GetWindowRect(fg, &mut r).is_err() {
        return false;
    }
    let m = mi.rcMonitor;
    r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
}

static TASKBAR_HIDDEN: AtomicBool = AtomicBool::new(false);

pub fn taskbar_hidden() -> bool {
    TASKBAR_HIDDEN.load(Relaxed)
}

/// Cache completement les barres des taches Windows (masquage auto active pour liberer l'ecran),
/// ou les reaffiche.
pub fn set_taskbar_hidden(hidden: bool) {
    TASKBAR_HIDDEN.store(hidden, Relaxed);
    if hidden {
        set_taskbar_autohide(true);
    }
    show_taskbars(!hidden);
}

/// Explorer reaffiche parfois sa barre (redemarrage, changement d'ecran) : on la recache.
fn enforce_taskbar() {
    if TASKBAR_HIDDEN.load(Relaxed) {
        show_taskbars(false);
    }
}

fn show_taskbars(show: bool) {
    unsafe {
        let mut bars = vec![tray::explorer_tray()];
        let mut after: Option<HWND> = None;
        while let Ok(h) = FindWindowExW(None, after, w!("Shell_SecondaryTrayWnd"), PCWSTR::null()) {
            if h.is_invalid() {
                break;
            }
            bars.push(h);
            after = Some(h);
        }
        for h in bars.into_iter().filter(|h| !h.is_invalid()) {
            if IsWindowVisible(h).as_bool() != show {
                let _ = ShowWindowAsync(h, if show { SW_SHOWNOACTIVATE } else { SW_HIDE });
            }
        }
    }
}

pub fn taskbar_autohide() -> bool {
    let mut d = data(HWND::default());
    unsafe { (SHAppBarMessage(ABM_GETSTATE, &mut d) as u32 & ABS_AUTOHIDE) != 0 }
}

pub fn set_taskbar_autohide(on: bool) {
    let mut d = data(tray::explorer_tray());
    d.lParam = LPARAM(if on { ABS_AUTOHIDE } else { ABS_ALWAYSONTOP } as isize);
    unsafe {
        SHAppBarMessage(ABM_SETSTATE, &mut d);
    }
}
