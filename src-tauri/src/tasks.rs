//! Fenetres ouvertes (equivalent barre des taches), pilote par evenements Windows.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use windows::core::{Interface, BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, FALSE, HWND, LPARAM, TRUE, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_MENU;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumChildWindows, EnumWindows, GetAncestor, GetForegroundWindow, GetMessageW, GetWindow,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    PostMessageW, SetForegroundWindow, ShowWindow, TranslateMessage, CHILDID_SELF, EVENT_OBJECT_CLOAKED,
    EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_UNCLOAKED,
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, GA_ROOT, GWL_EXSTYLE, GW_OWNER,
    MSG, OBJID_WINDOW, SW_MINIMIZE, SW_RESTORE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_CLOSE,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use crate::{appbar, icons, publish, sys, ConfigState};

#[derive(Serialize, Clone, PartialEq)]
struct Win {
    hwnd: isize,
    key: String,
    title: String,
    minimized: bool,
}

#[derive(Serialize, PartialEq)]
struct Tasks {
    windows: Vec<Win>,
    active: isize,
}

#[derive(Serialize, Clone, PartialEq)]
struct AppInfo {
    name: String,
    path: String,
    icon: Option<String>,
}

/// Epingle : `path` sert a lancer (exe ou raccourci), `key` a reconnaitre ses fenetres (exe cible).
#[derive(Serialize, Clone, PartialEq)]
struct Pin {
    path: String,
    key: String,
}

enum Ev {
    Dirty,
    Foreground(isize),
}

static TX: OnceLock<Sender<Ev>> = OnceLock::new();
/// Derniere fenetre active qui n'est pas la barre.
static ACTIVE: AtomicIsize = AtomicIsize::new(0);

pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel();
    let _ = TX.set(tx);
    std::thread::spawn(hook_thread);
    std::thread::spawn(move || worker(app, rx));
    // Filet de securite pour le plein ecran (F11 dans une meme fenetre, sans changement de focus).
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_millis(800));
        appbar::check_fullscreen();
    });
}

/// Force un rafraichissement (ex. apres modification des epingles).
pub fn poke() {
    if let Some(tx) = TX.get() {
        let _ = tx.send(Ev::Dirty);
    }
}

fn set_active(h: HWND) {
    if !h.is_invalid() && h != appbar::hwnd() {
        ACTIVE.store(h.0 as isize, Relaxed);
    }
}

#[derive(Default)]
struct Cache {
    apps: HashMap<String, AppInfo>,
    /// pid -> exe auquel ses fenetres sont rattachees.
    exe_by_pid: HashMap<u32, String>,
    pins: HashSet<String>,
    /// raccourci -> exe cible
    links: HashMap<String, Option<String>>,
}

fn worker(app: AppHandle, rx: Receiver<Ev>) {
    // Raccourcis (IShellLink) et icones du shell : COM requis sur ce thread.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let mut cache = Cache::default();
    set_active(unsafe { GetForegroundWindow() });
    let handle = |ev: Ev| {
        if let Ev::Foreground(h) = ev {
            set_active(HWND(h as *mut c_void));
            appbar::check_fullscreen();
        }
    };
    loop {
        refresh(&app, &mut cache);
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(ev) => {
                handle(ev);
                // Regroupe les rafales d'evenements (ouverture d'app = dizaines d'evenements).
                let end = Instant::now() + Duration::from_millis(90);
                while let Some(left) = end.checked_duration_since(Instant::now()) {
                    match rx.recv_timeout(left) {
                        Ok(ev) => handle(ev),
                        Err(_) => break,
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn refresh(app: &AppHandle, cache: &mut Cache) {
    let pinned = app.state::<ConfigState>().0.lock().unwrap().pinned.clone();
    let pins: Vec<Pin> = pinned
        .iter()
        .map(|p| {
            let target = pin_target(p, &mut cache.links);
            Pin { path: p.clone(), key: target.unwrap_or_else(|| p.clone()).to_lowercase() }
        })
        .collect();
    let keys: HashSet<String> = pins.iter().map(|p| p.key.clone()).collect();
    if keys != cache.pins {
        cache.pins = keys;
        cache.exe_by_pid.clear();
    }
    let mut new_apps = false;
    for pin in &pins {
        if !cache.apps.contains_key(&pin.key) {
            cache.apps.insert(pin.key.clone(), pin_info(pin));
            new_apps = true;
        }
    }
    publish(app, "pins", &pins);

    let mut handles: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }
    let mut seen = HashSet::new();
    let mut windows = Vec::new();
    for h in handles {
        if !unsafe { is_task(h) } {
            continue;
        }
        let pid = unsafe { real_pid(h) };
        seen.insert(pid);
        let exe = cache.exe_by_pid.entry(pid).or_insert_with(|| owning_exe(pid, &cache.pins)).clone();
        let key = if exe.is_empty() { format!("hwnd:{}", h.0 as isize) } else { exe.to_lowercase() };
        if !cache.apps.contains_key(&key) {
            cache.apps.insert(key.clone(), app_info(&exe, Some(h)));
            new_apps = true;
        }
        windows.push(Win { hwnd: h.0 as isize, key, title: title(h), minimized: unsafe { IsIconic(h).as_bool() } });
    }
    cache.exe_by_pid.retain(|pid, _| seen.contains(pid));

    if new_apps {
        publish(app, "apps", &cache.apps);
    }
    publish(app, "tasks", &Tasks { windows, active: ACTIVE.load(Relaxed) });
}

/// Exe vise par une epingle (cible du raccourci), ou None si ce n'est pas un raccourci resolu.
fn pin_target(path: &str, links: &mut HashMap<String, Option<String>>) -> Option<String> {
    if !path.to_lowercase().ends_with(".lnk") {
        return None;
    }
    links
        .entry(path.to_string())
        .or_insert_with(|| {
            link_target(path).or_else(|| {
                // "Explorateur de fichiers" pointe vers un emplacement du shell, pas vers un exe.
                let stem = Path::new(path).file_stem()?.to_string_lossy().to_lowercase();
                (stem.contains("explor")).then(|| format!(r"{}\explorer.exe", windir()))
            })
        })
        .clone()
}

fn link_target(lnk: &str) -> Option<String> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        let w = sys::wide(lnk);
        file.Load(PCWSTR(w.as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
        let target = String::from_utf16_lossy(&buf[..n]);
        (!target.is_empty() && Path::new(&target).exists()).then_some(target)
    }
}

fn pin_info(pin: &Pin) -> AppInfo {
    let lnk = pin.path.to_lowercase().ends_with(".lnk");
    let target = (pin.key != pin.path.to_lowercase()).then_some(pin.key.as_str());
    let mut info = app_info(target.unwrap_or(&pin.path), None);
    if lnk {
        if let Some(stem) = Path::new(&pin.path).file_stem() {
            info.name = stem.to_string_lossy().to_string();
        }
        // L'icone choisie par le raccourci (ex. Discord, dont la cible Update.exe a une icone generique).
        if let Some(icon) = icons::from_path(&pin.path) {
            info.icon = Some(icon);
        }
    }
    info.path = pin.path.clone();
    info
}

fn windir() -> String {
    std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into())
}

/// Dossier d'une app epinglee dont les autres exe lui sont rattaches (assistants, versions).
/// Exclut Windows et les dossiers trop generaux (Program Files...).
fn pin_dir(key: &str) -> Option<String> {
    if !key.ends_with(".exe") {
        return None;
    }
    let dir = Path::new(key).parent()?;
    let d = format!("{}\\", dir.to_string_lossy().trim_end_matches('\\'));
    (dir.components().count() >= 4 && !d.starts_with(&windir().to_lowercase())).then_some(d)
}

/// Exe auquel rattacher les fenetres d'un process. Un exe installe sous le dossier d'une app
/// epinglee lui est rattache (steamwebhelper -> Steam, Discord\app-1.0\Discord.exe -> Discord).
fn owning_exe(pid: u32, pins: &HashSet<String>) -> String {
    let exe = exe_path(pid);
    let lower = exe.to_lowercase();
    if exe.is_empty() || pins.contains(&lower) {
        return exe;
    }
    pins.iter()
        .filter(|p| pin_dir(p).is_some_and(|d| lower.starts_with(&d)))
        .max_by_key(|p| p.len())
        .cloned()
        .unwrap_or(exe)
}

fn app_info(path: &str, h: Option<HWND>) -> AppInfo {
    let stem = Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string());
    let name = sys::file_description(path)
        .or(stem)
        .or_else(|| h.map(title))
        .unwrap_or_else(|| "Application".into());
    let icon = (!path.is_empty()).then(|| icons::from_path(path)).flatten().or_else(|| h.and_then(icons::from_window));
    AppInfo { name, path: path.to_string(), icon }
}

unsafe extern "system" fn collect(h: HWND, lp: LPARAM) -> BOOL {
    (*(lp.0 as *mut Vec<HWND>)).push(h);
    TRUE
}

/// Regle "Alt+Tab" simplifiee : fenetres visibles, non possedees, non outils, non masquees (cloaked).
unsafe fn is_task(h: HWND) -> bool {
    if !IsWindowVisible(h).as_bool() || h == appbar::hwnd() {
        return false;
    }
    let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
    let app_window = ex & WS_EX_APPWINDOW.0 != 0;
    if !app_window {
        if ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0 {
            return false;
        }
        if GetWindow(h, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
            return false;
        }
    }
    let mut cloaked = 0u32;
    let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4);
    if cloaked != 0 || GetWindowTextLengthW(h) == 0 {
        return false;
    }
    !matches!(
        sys::class_name(h).as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Windows.UI.Core.CoreWindow"
    )
}

fn title(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Les apps UWP sont hebergees par ApplicationFrameHost : on cherche le vrai process.
unsafe fn real_pid(h: HWND) -> u32 {
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    if sys::class_name(h) == "ApplicationFrameWindow" {
        let mut data = (pid, 0u32);
        let _ = EnumChildWindows(Some(h), Some(uwp_child), LPARAM(&mut data as *mut _ as isize));
        if data.1 != 0 {
            return data.1;
        }
    }
    pid
}

unsafe extern "system" fn uwp_child(c: HWND, lp: LPARAM) -> BOOL {
    let data = &mut *(lp.0 as *mut (u32, u32));
    let mut pid = 0u32;
    GetWindowThreadProcessId(c, Some(&mut pid));
    if pid != data.0 {
        data.1 = pid;
        return FALSE;
    }
    TRUE
}

pub fn exe_path(pid: u32) -> String {
    unsafe {
        let Ok(proc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(proc, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(proc);
        if ok {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        }
    }
}

/// Clic sur une fenetre : la ramene, ou la reduit si elle est deja au premier plan.
pub fn activate(hwnd: isize) {
    let h = HWND(hwnd as *mut c_void);
    unsafe {
        if ACTIVE.load(Relaxed) == hwnd && !IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_MINIMIZE);
            return;
        }
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        if !SetForegroundWindow(h).as_bool() {
            // Verrou de premier plan : une frappe Alt factice debloque SetForegroundWindow.
            sys::press(&[VK_MENU]);
            let _ = SetForegroundWindow(h);
        }
    }
}

pub fn close(hwnd: isize) {
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut c_void)), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    h: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if id_object != OBJID_WINDOW.0 || id_child != CHILDID_SELF as i32 || h.is_invalid() {
        return;
    }
    // Ignore les sous-controles (champs de texte, etc.) sauf a la destruction (fenetre deja invalide).
    if event != EVENT_OBJECT_DESTROY && GetAncestor(h, GA_ROOT) != h {
        return;
    }
    let ev = if event == EVENT_SYSTEM_FOREGROUND { Ev::Foreground(h.0 as isize) } else { Ev::Dirty };
    if let Some(tx) = TX.get() {
        let _ = tx.send(ev);
    }
}

fn hook_thread() {
    let ranges = [
        (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
        (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
        (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
        (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
        (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
    ];
    unsafe {
        for (min, max) in ranges {
            SetWinEventHook(min, max, None, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
