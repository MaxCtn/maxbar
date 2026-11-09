//! Géométrie de la barre.
//!
//! - Une bande de l'écran est réservée via l'API "AppBar" : les fenêtres maximisées s'arrêtent au bord.
//! - La fenêtre couvre tout l'écran (moins 1 px, pour ne pas être prise pour une app plein écran),
//!   mais une *région* Win32 limite la zone visible et cliquable à la barre + aux éléments ouverts
//!   (îlot, panneaux, volet). Le reste est invisible et laisse passer les clics.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_CLOAKED,
    DWMWA_EXTENDED_FRAME_BOUNDS, DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    DWMWINDOWATTRIBUTE,
};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, GetMonitorInfoW, MonitorFromWindow, SetWindowRgn,
    HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST, RGN_OR,
};
use windows::Win32::UI::Shell::{
    DefSubclassProc, SHAppBarMessage, SetWindowSubclass, ABE_BOTTOM, ABE_TOP, ABM_GETSTATE, ABM_NEW, ABM_QUERYPOS,
    ABM_REMOVE, ABM_SETPOS, ABM_SETSTATE, ABN_POSCHANGED, ABS_ALWAYSONTOP, ABS_AUTOHIDE, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, IsIconic, IsWindowVisible, IsZoomed,
    RegisterWindowMessageW, SetWindowLongPtrW, SetWindowPos, ShowWindowAsync, GWL_EXSTYLE, GWL_STYLE, HWND_TOPMOST,
    SWP_ASYNCWINDOWPOS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
    SW_SHOWNOACTIVATE, WM_APP, WM_NCACTIVATE, WM_NCPAINT, WS_CAPTION, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
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
/// Bande réservée et rectangle de la fenêtre, en pixels physiques écran.
static BAND: Mutex<RECT> = Mutex::new(ZERO);
static WIN: Mutex<RECT> = Mutex::new(ZERO);
static EXTRA: Mutex<Vec<Shape>> = Mutex::new(Vec::new());

/// Rectangle arrondi en px CSS, relatif à la fenêtre (envoyé par l'interface).
#[derive(Deserialize, Clone, Copy, PartialEq)]
pub struct Shape {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
}

/// Position de la barre dans la fenêtre, en px CSS.
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
    let h = HWND(win.hwnd()?.0);
    BAR.store(h.0 as isize, Relaxed);
    EXPLORER.store(tray::explorer_tray().0 as isize, Relaxed);
    unsafe {
        strip_frame(h);
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
    // 1 px de moins que l'écran, du côté opposé à la barre.
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
        let _ =
            SetWindowPos(hwnd(), Some(HWND_TOPMOST), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOACTIVATE);
    }
}

/// Zones visibles en plus de la barre (îlot déployé, panneaux, volet latéral).
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
        let region =
            CreateRectRgn(band.left - win.left, band.top - win.top, band.right - win.left, band.bottom - win.top);
        for sh in shapes.iter().filter(|sh| sh.w > 0.0 && sh.h > 0.0) {
            let (x, y) = (px(sh.x), px(sh.y));
            let d = px(sh.r) * 2;
            let part = CreateRoundRectRgn(x, y, x + px(sh.w) + 1, y + px(sh.h) + 1, d, d);
            CombineRgn(Some(region), Some(region), Some(part), RGN_OR);
            let _ = DeleteObject(HGDIOBJ(part.0));
        }
        // Le système devient propriétaire de la région.
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
        set_taskbar_hidden(false);
    }
    if REGISTERED.swap(false, Relaxed) {
        let mut d = data(hwnd());
        unsafe {
            SHAppBarMessage(ABM_REMOVE, &mut d);
        }
    }
}

/// Retire tout cadre Windows. Sans ça, une fenêtre transparente sans bordure reçoit une barre de titre
/// blanche (titre + bouton fermer) quand elle prend le focus.
unsafe fn strip_frame(h: HWND) {
    let frame = WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
    let style = (GetWindowLongPtrW(h, GWL_STYLE) as u32 & !frame.0) | WS_POPUP.0;
    SetWindowLongPtrW(h, GWL_STYLE, style as isize);
    let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
    SetWindowLongPtrW(h, GWL_EXSTYLE, ex | WS_EX_TOOLWINDOW.0 as isize);

    let set = |attr: DWMWINDOWATTRIBUTE, value: u32| {
        let _ = DwmSetWindowAttribute(h, attr, &value as *const u32 as *const c_void, 4);
    };
    set(DWMWA_NCRENDERING_POLICY, DWMNCRP_DISABLED.0 as u32);
    set(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND.0 as u32);
    set(DWMWA_BORDER_COLOR, 0xFFFF_FFFE); // DWMWA_COLOR_NONE : pas de liseré Windows 11
    let flags = SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
    let _ = SetWindowPos(h, None, 0, 0, 0, 0, flags);
}

unsafe extern "system" fn subclass(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match msg {
        // lParam = -1 : changer l'état actif sans redessiner de cadre.
        WM_NCACTIVATE => return DefSubclassProc(h, msg, wp, LPARAM(-1)),
        WM_NCPAINT => return LRESULT(0),
        _ => {}
    }
    if msg == CALLBACK_MSG && wp.0 as u32 == ABN_POSCHANGED {
        reposition();
    } else if msg != 0 && msg == TASKBAR_CREATED.load(Relaxed) {
        // Vrai redémarrage d'Explorer (pas notre propre diffusion) : il a oublié notre AppBar.
        let current = tray::explorer_tray().0 as isize;
        if EXPLORER.swap(current, Relaxed) != current {
            REGISTERED.store(false, Relaxed);
            reposition();
        }
    }
    DefSubclassProc(h, msg, wp, lp)
}

/// Vrai quand une app plein écran (jeu, vidéo) est au premier plan.
pub fn fullscreen_active() -> bool {
    HIDDEN.load(Relaxed)
}

/// Cache la barre quand l'app au premier plan est en plein écran (jeu, vidéo).
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
    if is_shell_window(fg) {
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

/// Appelé à chaque changement de fenêtre active.
pub fn on_foreground(fg: HWND) {
    let bar = hwnd();
    if bar.is_invalid() || fg.is_invalid() || unsafe { is_fullscreen(fg, bar) } {
        return;
    }
    // Une autre fenêtre "toujours au-dessus" activée passe devant nous : on reprend la main.
    unsafe {
        let _ = SetWindowPos(bar, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
    keep_clear(fg);
}

/// Décale sous la barre une fenêtre dont la barre de titre se retrouve dessous
/// (fenêtre ouverte en haut de l'écran ou lâchée contre le bord).
pub fn keep_clear(h: HWND) {
    let bar = hwnd();
    if bar.is_invalid() || h == bar || BOTTOM.load(Relaxed) || HIDDEN.load(Relaxed) {
        return;
    }
    unsafe {
        if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() || IsZoomed(h).as_bool() {
            return;
        }
        let style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        let framed = style & WS_CAPTION.0 == WS_CAPTION.0 || style & WS_THICKFRAME.0 != 0;
        if !framed || style & WS_CHILD.0 != 0 || ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0 {
            return;
        }
        if dwm_attr::<u32>(h, DWMWA_CLOAKED).unwrap_or(0) != 0 || is_shell_window(h) || is_fullscreen(h, bar) {
            return;
        }
        let Some(frame) = dwm_attr::<RECT>(h, DWMWA_EXTENDED_FRAME_BOUNDS) else { return };
        let band = *BAND.lock().unwrap();
        let overlaps = frame.left < band.right && frame.right > band.left && frame.top < band.bottom;
        let mut r = RECT::default();
        if !overlaps || frame.bottom <= band.top || GetWindowRect(h, &mut r).is_err() {
            return;
        }
        let dy = band.bottom - frame.top;
        let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS;
        let _ = SetWindowPos(h, None, r.left, r.top + dy, 0, 0, flags);
    }
}

unsafe fn dwm_attr<T: Default>(h: HWND, attr: DWMWINDOWATTRIBUTE) -> Option<T> {
    let mut value = T::default();
    let size = std::mem::size_of::<T>() as u32;
    DwmGetWindowAttribute(h, attr, &mut value as *mut T as *mut c_void, size).ok()?;
    Some(value)
}

fn is_shell_window(h: HWND) -> bool {
    matches!(
        class_name(h).as_str(),
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "Windows.UI.Core.CoreWindow"
            | "XamlExplorerHostIslandWindow"
            | "TopLevelWindowForOverflowXamlIsland"
            | "NotifyIconOverflowWindow"
    )
}

static TASKBAR_HIDDEN: AtomicBool = AtomicBool::new(false);
/// Le masquage auto a été activé par nous : à désactiver quand on rend la barre.
static AUTOHIDE_BY_US: AtomicBool = AtomicBool::new(false);

pub fn taskbar_hidden() -> bool {
    TASKBAR_HIDDEN.load(Relaxed)
}

/// Cache complètement les barres des tâches Windows (masquage auto activé pour libérer l'écran),
/// ou les réaffiche dans leur état d'origine.
pub fn set_taskbar_hidden(hidden: bool) {
    TASKBAR_HIDDEN.store(hidden, Relaxed);
    if hidden && !taskbar_autohide() {
        set_taskbar_autohide(true);
        AUTOHIDE_BY_US.store(true, Relaxed);
    }
    show_taskbars(!hidden);
    if !hidden && AUTOHIDE_BY_US.swap(false, Relaxed) {
        set_taskbar_autohide(false);
    }
}

/// Explorer réaffiche parfois sa barre (redémarrage, changement d'écran) : on la recache.
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
