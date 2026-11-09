//! Zone de notification (icones des applications en arriere-plan : Discord, Steam, OneDrive...).
//!
//! Les applications deposent leurs icones aupres de la fenetre "Shell_TrayWnd" (Shell_NotifyIcon
//! -> WM_COPYDATA). maxbar cree une fenetre cachee de la meme classe, maintenue devant celle
//! d'Explorer : elle lit chaque message puis le retransmet tel quel a Explorer, dont la barre
//! continue de fonctionner normalement. Meme principe que Zebar (crate systray-util).

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tauri::AppHandle;
use windows::core::{w, GUID, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowExW, FindWindowW, GetCursorPos,
    GetMessageW, GetWindowRect, GetWindowThreadProcessId, IsWindow, KillTimer, PostMessageW, RegisterClassW,
    RegisterWindowMessageW, SendMessageTimeoutW, SendNotifyMessageW, SetTimer, SetWindowPos, TranslateMessage, HICON,
    HWND_BROADCAST, HWND_TOPMOST, MSG, SMTO_ABORTIFHUNG, SMTO_NORMAL, SWP_NOACTIVATE, SWP_NOOWNERZORDER,
    SWP_NOSENDCHANGING, WM_COMMAND, WM_CONTEXTMENU, WM_COPYDATA, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_TIMER, WM_USER, WNDCLASSW, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::{icons, publish, sys, tasks};

const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIM_SETVERSION: u32 = 4;
const NIF_MESSAGE: u32 = 0x1;
const NIF_ICON: u32 = 0x2;
const NIF_TIP: u32 = 0x4;
const NIF_STATE: u32 = 0x8;
const NIF_GUID: u32 = 0x20;
const NIS_HIDDEN: u32 = 0x1;
const NIN_SELECT: u32 = WM_USER;
const SIGNATURE: u32 = 0x3475_3423;

const T_FRONT: usize = 1;
const T_PRUNE: usize = 2;
const T_PUBLISH: usize = 3;
const T_REFRESH: usize = 4;

/// NOTIFYICONDATAW tel que transmis par le shell : poignees sur 32 bits, meme en 64 bits.
#[repr(C)]
#[derive(Clone, Copy)]
struct NotifyData {
    size: u32,
    hwnd: u32,
    uid: u32,
    flags: u32,
    callback: u32,
    icon: u32,
    tip: [u16; 128],
    state: u32,
    state_mask: u32,
    info: [u16; 256],
    version: u32,
    info_title: [u16; 64],
    info_flags: u32,
    guid: GUID,
    balloon_icon: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TrayMessage {
    signature: u32,
    message: u32,
    data: NotifyData,
}

#[derive(Default)]
struct Icon {
    hwnd: isize,
    uid: u32,
    guid: Option<GUID>,
    callback: u32,
    version: u32,
    tip: String,
    image: Option<String>,
    hidden: bool,
    app: String,
}

impl Icon {
    fn id(&self) -> String {
        match self.guid {
            Some(g) => format!("{g:?}"),
            None => format!("{}-{}", self.hwnd, self.uid),
        }
    }
}

#[derive(Serialize, PartialEq)]
struct TrayIcon {
    id: String,
    tip: String,
    icon: String,
    app: String,
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static ICONS: Mutex<Vec<Icon>> = Mutex::new(Vec::new());
static SPY: AtomicIsize = AtomicIsize::new(0);
static EXPLORER: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

pub fn start(app: AppHandle) {
    let _ = APP.set(app);
    std::thread::spawn(|| unsafe { run() });
}

/// La vraie barre des taches d'Explorer (en ignorant notre propre fenetre "Shell_TrayWnd").
pub fn explorer_tray() -> HWND {
    let me = unsafe { GetCurrentProcessId() };
    let mut after: Option<HWND> = None;
    unsafe {
        while let Ok(h) = FindWindowExW(None, after, w!("Shell_TrayWnd"), PCWSTR::null()) {
            if h.is_invalid() {
                break;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid != me {
                return h;
            }
            after = Some(h);
        }
    }
    HWND::default()
}

unsafe fn run() {
    let Ok(module) = GetModuleHandleW(None) else { return };
    let instance = HINSTANCE(module.0);
    let class = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: instance,
        lpszClassName: w!("Shell_TrayWnd"),
        ..Default::default()
    };
    if RegisterClassW(&class) == 0 {
        return;
    }
    let Ok(h) = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
        w!("Shell_TrayWnd"),
        PCWSTR::null(),
        WS_POPUP,
        0,
        0,
        0,
        0,
        None,
        None,
        Some(instance),
        None,
    ) else {
        return;
    };
    SPY.store(h.0 as isize, Relaxed);
    TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Relaxed);
    EXPLORER.store(explorer_tray().0 as isize, Relaxed);
    stay_in_front(h);
    SetTimer(Some(h), T_FRONT, 100, None);
    SetTimer(Some(h), T_PRUNE, 5000, None);
    // Comme apres un redemarrage d'Explorer : chaque application redeclare ses icones.
    broadcast();
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

unsafe fn broadcast() {
    let _ = SendNotifyMessageW(HWND_BROADCAST, TASKBAR_CREATED.load(Relaxed), WPARAM(0), LPARAM(0));
}

/// Garde notre fenetre devant celle d'Explorer (FindWindow renvoie la premiere dans l'ordre Z)
/// et a la meme place, pour les applications qui lisent la position de la barre.
unsafe fn stay_in_front(h: HWND) {
    let explorer = explorer_tray();
    let mut want = RECT::default();
    if !explorer.is_invalid() {
        let _ = GetWindowRect(explorer, &mut want);
    }
    let mut have = RECT::default();
    let _ = GetWindowRect(h, &mut have);
    if FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).ok() == Some(h) && have == want {
        return;
    }
    let _ = SetWindowPos(
        h,
        Some(HWND_TOPMOST),
        want.left,
        want.top,
        want.right - want.left,
        want.bottom - want.top,
        SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOSENDCHANGING,
    );
}

unsafe fn forward(msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let target = explorer_tray();
    if target.is_invalid() {
        return LRESULT(0);
    }
    let mut res = 0usize;
    SendMessageTimeoutW(target, msg, wp, lp, SMTO_ABORTIFHUNG | SMTO_NORMAL, 5000, Some(&mut res));
    LRESULT(res as isize)
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COPYDATA if lp.0 != 0 => {
            let cds = &*(lp.0 as *const COPYDATASTRUCT);
            match cds.dwData {
                1 => on_notify(h, cds),
                // Shell_NotifyIconGetRect : on repond la position du curseur (icone affichee chez nous).
                3 => return anchor(cds),
                _ => {}
            }
            forward(msg, wp, lp)
        }
        WM_TIMER => {
            on_timer(h, wp.0);
            LRESULT(0)
        }
        WM_COMMAND => forward(msg, wp, lp),
        m if m != 0 && m == TASKBAR_CREATED.load(Relaxed) => {
            // Vrai redemarrage d'Explorer : on se replace devant lui puis on redemande les icones.
            let current = explorer_tray().0 as isize;
            if EXPLORER.swap(current, Relaxed) != current {
                stay_in_front(h);
                SetTimer(Some(h), T_REFRESH, 1500, None);
            }
            LRESULT(0)
        }
        m if m >= WM_USER => {
            let target = explorer_tray();
            if !target.is_invalid() {
                let _ = PostMessageW(Some(target), m, wp, lp);
            }
            DefWindowProcW(h, msg, wp, lp)
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

unsafe fn on_timer(h: HWND, id: usize) {
    match id {
        T_FRONT => stay_in_front(h),
        T_PRUNE => {
            let mut list = ICONS.lock().unwrap();
            let before = list.len();
            list.retain(|i| IsWindow(Some(HWND(i.hwnd as *mut c_void))).as_bool());
            if list.len() != before {
                drop(list);
                publish_icons();
            }
        }
        T_PUBLISH => {
            let _ = KillTimer(Some(h), T_PUBLISH);
            publish_icons();
        }
        T_REFRESH => {
            let _ = KillTimer(Some(h), T_REFRESH);
            broadcast();
        }
        _ => {}
    }
}

unsafe fn anchor(cds: &COPYDATASTRUCT) -> LRESULT {
    let kind = if cds.lpData.is_null() || cds.cbData < 8 { 0 } else { *(cds.lpData as *const u32).add(1) };
    let mut p = POINT::default();
    let _ = GetCursorPos(&mut p);
    let y = if kind == 2 { p.y + 1 } else { p.y };
    match kind {
        1 | 2 => LRESULT((((y as u16 as u32) << 16) | p.x as u16 as u32) as isize),
        _ => LRESULT(0),
    }
}

unsafe fn on_notify(h: HWND, cds: &COPYDATASTRUCT) {
    if cds.lpData.is_null() || (cds.cbData as usize) < 32 {
        return;
    }
    let mut m: TrayMessage = std::mem::zeroed();
    let n = (cds.cbData as usize).min(std::mem::size_of::<TrayMessage>());
    std::ptr::copy_nonoverlapping(cds.lpData as *const u8, &mut m as *mut TrayMessage as *mut u8, n);
    if m.signature == SIGNATURE && apply(m.message, &m.data) {
        // Regroupe les rafales (icones animees, ajout de toutes les icones au demarrage).
        SetTimer(Some(h), T_PUBLISH, 150, None);
    }
}

fn text(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).replace('\r', "").trim().to_string()
}

fn owner_name(hwnd: isize) -> String {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut c_void), Some(&mut pid)) };
    let exe = tasks::exe_path(pid);
    sys::file_description(&exe)
        .or_else(|| std::path::Path::new(&exe).file_stem().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_default()
}

/// Applique un message Shell_NotifyIcon ; renvoie vrai si l'affichage change.
fn apply(message: u32, d: &NotifyData) -> bool {
    let guid = (d.flags & NIF_GUID != 0 && d.guid != GUID::zeroed()).then_some(d.guid);
    let hwnd = d.hwnd as i32 as isize;
    let mut list = ICONS.lock().unwrap();
    let pos = list.iter().position(|i| match guid {
        Some(g) => i.guid == Some(g),
        None => i.guid.is_none() && i.hwnd == hwnd && i.uid == d.uid,
    });
    match message {
        NIM_DELETE => pos.map(|p| list.remove(p)).is_some(),
        NIM_SETVERSION => {
            if let Some(p) = pos {
                list[p].version = d.version;
            }
            false
        }
        NIM_ADD | NIM_MODIFY => {
            let p = pos.unwrap_or_else(|| {
                list.push(Icon { hwnd, uid: d.uid, guid, app: owner_name(hwnd), ..Default::default() });
                list.len() - 1
            });
            let icon = &mut list[p];
            let before = (icon.tip.clone(), icon.image.clone(), icon.hidden);
            if hwnd != 0 {
                icon.hwnd = hwnd;
            }
            icon.uid = d.uid;
            if d.flags & NIF_MESSAGE != 0 {
                icon.callback = d.callback;
            }
            if d.flags & NIF_ICON != 0 {
                if d.icon == 0 {
                    icon.image = None;
                } else if let Some(img) = icons::encode_icon(HICON(d.icon as i32 as isize as *mut c_void)) {
                    icon.image = Some(img);
                }
            }
            if d.flags & NIF_TIP != 0 {
                icon.tip = text(&d.tip);
            }
            if d.flags & NIF_STATE != 0 && d.state_mask & NIS_HIDDEN != 0 {
                icon.hidden = d.state & NIS_HIDDEN != 0;
            }
            pos.is_none() || before != (icon.tip.clone(), icon.image.clone(), icon.hidden)
        }
        _ => false,
    }
}

fn publish_icons() {
    let Some(app) = APP.get() else { return };
    let out: Vec<TrayIcon> = ICONS
        .lock()
        .unwrap()
        .iter()
        .filter(|i| !i.hidden)
        .filter_map(|i| {
            Some(TrayIcon {
                id: i.id(),
                tip: if i.tip.is_empty() { i.app.clone() } else { i.tip.clone() },
                icon: i.image.clone()?,
                app: i.app.clone(),
            })
        })
        .collect();
    publish(app, "tray", &out);
}

/// Clic sur une icone, transmis a son application comme le ferait la barre des taches.
pub fn click(id: &str, button: &str) {
    let found = ICONS
        .lock()
        .unwrap()
        .iter()
        .find(|i| i.id() == id)
        .map(|i| (HWND(i.hwnd as *mut c_void), i.uid, i.callback, i.version));
    let Some((h, uid, callback, version)) = found else { return };
    unsafe {
        if callback == 0 || !IsWindow(Some(h)).as_bool() {
            return;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        // Autorise l'application a passer au premier plan (son menu doit pouvoir prendre le focus).
        let _ = AllowSetForegroundWindow(pid);
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        let send = |m: u32| {
            let (wp, lp) = if version > 3 {
                (((p.y as u16 as usize) << 16) | p.x as u16 as usize, ((uid as u16 as isize) << 16) | m as u16 as isize)
            } else {
                (uid as usize, m as isize)
            };
            let _ = SendNotifyMessageW(h, callback, WPARAM(wp), LPARAM(lp));
        };
        let modern = version >= 3;
        match button {
            "right" => {
                send(WM_RBUTTONDOWN);
                send(WM_RBUTTONUP);
                if modern {
                    send(WM_CONTEXTMENU);
                }
            }
            "middle" => {
                send(WM_MBUTTONDOWN);
                send(WM_MBUTTONUP);
            }
            "double" => {
                send(WM_LBUTTONDBLCLK);
                send(WM_LBUTTONUP);
                if modern {
                    send(NIN_SELECT);
                }
            }
            _ => {
                send(WM_LBUTTONDOWN);
                send(WM_LBUTTONUP);
                if modern {
                    send(NIN_SELECT);
                }
            }
        }
    }
}
