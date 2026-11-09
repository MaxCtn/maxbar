//! Touche Windows : ouvre le lanceur de maxbar à la place du menu Démarrer.
//!
//! Un hook clavier bas niveau suit la touche Win. Relâchée sans qu'aucune autre touche ni aucun clic ne soit
//! survenu entre-temps, elle ouvre (ou referme) le lanceur ; Win+S l'ouvre en mode recherche. Toutes les autres
//! combinaisons passent intactes. Pour que Windows n'ouvre pas son propre menu, une frappe factice est glissée
//! juste avant le relâchement : il croit alors à une combinaison.
//!
//! Windows retire sans prévenir un hook qui tarde à répondre : le callback ne manipule que des atomiques et
//! délègue tout le reste. En plein écran (jeu, vidéo, sur n'importe quel écran) ou si la fonction est désactivée,
//! le hook est désinstallé.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering::Relaxed};
use std::sync::OnceLock;
use std::thread::{self, Thread};

use tauri::{AppHandle, Emitter};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, MOUSEINPUT, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_S, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetMessageW, IsIconic, IsWindow, KillTimer, PeekMessageW, PostThreadMessageW,
    SetForegroundWindow, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, HHOOK, HOOKPROC, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, LLKHF_UP, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL, WINDOWS_HOOK_ID, WM_APP, WM_LBUTTONDOWN,
    WM_MBUTTONDOWN, WM_MOUSEHWHEEL, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_TIMER, WM_USER, WM_XBUTTONDOWN,
};

use crate::{appbar, tasks};

/// Signature de nos propres frappes injectées (dwExtraInfo) : le hook les laisse passer sans les compter.
const TAG: usize = 0x6D61_7862;
/// Touche virtuelle sans usage, envoyée pour « masquer » le relâchement de Win.
/// Même choix que PowerToys Keyboard Manager (DUMMY_KEY, licence MIT).
const DUMMY: VIRTUAL_KEY = VIRTUAL_KEY(0xFF);

const MSG_SYNC: u32 = WM_APP + 1;
const MSG_MOUSE: u32 = WM_APP + 2;
/// Renouvellement du hook, en tours de minuteur (une seconde chacun).
const RENEW_TICKS: u32 = 60;

/// Aucune touche Win suivie.
const IDLE: u8 = 0;
/// Win tenue seule : son relâchement ouvrira le lanceur.
const ALONE: u8 = 1;
/// Win tenue pendant un clic : Windows n'a vu aucune autre touche, il faut quand même masquer le relâchement.
const MASK: u8 = 2;
/// Combinaison que Windows a vue (Win+E, Ctrl+Win...) : on ne s'en mêle plus.
const CHORD: u8 = 3;

const TOGGLE: u8 = 1;
const SEARCH: u8 = 2;

static ENABLED: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
static STATE: AtomicU8 = AtomicU8::new(IDLE);
/// Touche Win (gauche ou droite) dont on suit l'appui.
static HELD: AtomicU32 = AtomicU32::new(0);
/// Le S de Win+S a été avalé : ses répétitions et son relâchement le seront aussi.
static S_EATEN: AtomicBool = AtomicBool::new(false);
static REQUEST: AtomicU8 = AtomicU8::new(0);
static WORKER: OnceLock<Thread> = OnceLock::new();

pub fn start(app: AppHandle) {
    if WORKER.get().is_some() {
        return;
    }
    let Ok(worker) = thread::Builder::new().name("winkey-launcher".into()).spawn(move || serve(app)) else {
        return;
    };
    let _ = WORKER.set(worker.thread().clone());
    let _ = thread::Builder::new().name("winkey-hook".into()).spawn(hook_thread);
}

/// Interception de la touche Win (config `win_key`). Désactivée, le hook est désinstallé.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Relaxed);
    post(MSG_SYNC, 0);
}

/// Rend le premier plan à la fenêtre active avant l'ouverture du lanceur. À appeler quand il se referme sans
/// rien lancer (Échap, nouvel appui sur Win) : sinon le clavier reste sur la barre, invisible.
#[tauri::command]
pub fn restore_focus() {
    let (bar, previous) = (appbar::hwnd(), tasks::active_window());
    unsafe {
        if GetForegroundWindow() == bar && IsWindow(Some(previous)).as_bool() && !IsIconic(previous).as_bool() {
            let _ = SetForegroundWindow(previous);
        }
    }
}

/// Ouvre le lanceur à la demande du hook, hors du callback.
fn serve(app: AppHandle) {
    loop {
        thread::park();
        let mode = match REQUEST.swap(0, Relaxed) {
            TOGGLE => "toggle",
            SEARCH => "search",
            _ => continue,
        };
        if !appbar::fullscreen_anywhere() {
            bring_to_front(appbar::hwnd());
            let _ = app.emit("launcher", mode);
        }
    }
}

/// Donne le premier plan à la barre ; wry passe ensuite le focus clavier à WebView2 (WM_SETFOCUS).
///
/// `WebviewWindow::set_focus()` ne ferait rien ici : tao croit la fenêtre invisible, puisqu'elle est affichée
/// par `appbar::show` et non par tao.
fn bring_to_front(bar: HWND) {
    if bar.is_invalid() {
        return;
    }
    unsafe {
        if GetForegroundWindow() == bar || SetForegroundWindow(bar).as_bool() {
            return;
        }
        // Astuce de PowerToys (FancyZones, licence MIT) : une entrée souris vide fait de nous le dernier
        // processus à avoir reçu une entrée, ce qui lève le verrou de premier plan.
        let nudge = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: MOUSEINPUT { dwExtraInfo: TAG, ..Default::default() } },
        };
        SendInput(&[nudge], size_of::<INPUT>() as i32);
        let _ = SetForegroundWindow(bar);
    }
}

fn hook_thread() {
    let mut hooks = Hooks::default();
    let mut msg = MSG::default();
    unsafe {
        // Toutes les frappes du système transitent par ce thread : il ne doit pas attendre son tour.
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
        // Crée la file de messages avant de publier l'identifiant, sinon PostThreadMessage échoue.
        let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
        HOOK_THREAD.store(GetCurrentThreadId(), Relaxed);
        hooks.sync();
        // Les callbacks des hooks sont appelés depuis GetMessageW.
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            match msg.message {
                WM_TIMER | MSG_SYNC => hooks.sync(),
                MSG_MOUSE => hooks.watch_mouse(msg.wParam.0 != 0),
                _ => {}
            }
        }
    }
}

#[derive(Default)]
struct Hooks {
    keyboard: Option<HHOOK>,
    /// Posé seulement tant que Win est tenue seule, pour repérer Win+clic sans ralentir la souris le reste du temps.
    mouse: Option<HHOOK>,
    timer: usize,
    age: u32,
}

impl Hooks {
    /// Installe ou retire le hook selon la config et le plein écran. Rappelé chaque seconde tant que la
    /// fonction est activée.
    fn sync(&mut self) {
        let enabled = ENABLED.load(Relaxed);
        let wanted = enabled && !appbar::fullscreen_anywhere();
        match self.keyboard {
            None if wanted => {
                self.forget();
                S_EATEN.store(false, Relaxed);
                self.keyboard = install(WH_KEYBOARD_LL, Some(keyboard_proc));
                self.age = 0;
            }
            Some(hook) if !wanted => {
                unhook(hook);
                self.keyboard = None;
                self.forget();
            }
            Some(hook) => self.renew(hook),
            None => {}
        }
        // Relâchement de Win jamais reçu (Win+L, bureau sécurisé...) : on repart de zéro.
        if STATE.load(Relaxed) != IDLE && !win_down() {
            self.forget();
        }
        self.set_timer(enabled);
    }

    /// Windows retire sans rien dire un hook qui a tardé une seule fois (sortie de veille, machine saturée) :
    /// on le renouvelle chaque minute, le nouveau avant l'ancien pour ne perdre aucune frappe.
    fn renew(&mut self, old: HHOOK) {
        self.age += 1;
        if self.age < RENEW_TICKS || STATE.load(Relaxed) != IDLE {
            return;
        }
        self.age = 0;
        if let Some(new) = install(WH_KEYBOARD_LL, Some(keyboard_proc)) {
            self.keyboard = Some(new);
            unhook(old);
        }
    }

    fn watch_mouse(&mut self, on: bool) {
        let on = on && self.keyboard.is_some() && STATE.load(Relaxed) == ALONE;
        match self.mouse {
            None if on => self.mouse = install(WH_MOUSE_LL, Some(mouse_proc)),
            Some(hook) if !on => {
                unhook(hook);
                self.mouse = None;
            }
            _ => {}
        }
    }

    /// Oublie l'appui en cours sur Win.
    fn forget(&mut self) {
        STATE.store(IDLE, Relaxed);
        HELD.store(0, Relaxed);
        self.watch_mouse(false);
    }

    fn set_timer(&mut self, on: bool) {
        unsafe {
            if on && self.timer == 0 {
                self.timer = SetTimer(None, 0, 1000, None);
            } else if !on && self.timer != 0 {
                let _ = KillTimer(None, self.timer);
                self.timer = 0;
            }
        }
    }
}

fn install(kind: WINDOWS_HOOK_ID, proc: HOOKPROC) -> Option<HHOOK> {
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        SetWindowsHookExW(kind, proc, Some(module.into()), 0).ok()
    }
}

fn unhook(hook: HHOOK) {
    unsafe {
        let _ = UnhookWindowsHookEx(hook);
    }
}

fn post(msg: u32, wparam: usize) {
    let thread = HOOK_THREAD.load(Relaxed);
    if thread != 0 {
        unsafe {
            let _ = PostThreadMessageW(thread, msg, WPARAM(wparam), LPARAM(0));
        }
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 && on_key(&*(lparam.0 as *const KBDLLHOOKSTRUCT)) {
        return LRESULT(1);
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let press = matches!(
        wparam.0 as u32,
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN | WM_MOUSEWHEEL | WM_MOUSEHWHEEL
    );
    if code == HC_ACTION as i32 && press && STATE.compare_exchange(ALONE, MASK, Relaxed, Relaxed).is_ok() {
        post(MSG_MOUSE, 0);
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Décide du sort d'une frappe : vrai pour l'avaler. Appelé dans le callback : rien qui bloque ou alloue.
fn on_key(k: &KBDLLHOOKSTRUCT) -> bool {
    let (vk, up) = (k.vkCode, k.flags.contains(LLKHF_UP));
    if k.dwExtraInfo == TAG {
        return false;
    }
    if vk == VK_S.0 as u32 && up {
        return S_EATEN.swap(false, Relaxed);
    }
    if !ENABLED.load(Relaxed) || appbar::fullscreen_anywhere() {
        return false;
    }
    // Une frappe injectée par un autre logiciel compte comme une touche intermédiaire, comme pour Windows ;
    // une touche Win injectée (sys::press, remappeur, clavier visuel) garde son comportement natif.
    let physical = !k.flags.contains(LLKHF_INJECTED);
    let state = STATE.load(Relaxed);

    if vk == VK_S.0 as u32 && physical {
        if S_EATEN.load(Relaxed) {
            return true; // répétition automatique du S avalé
        }
        // Avec Maj, Ctrl ou Alt, ce n'est plus Win+S (Win+Maj+S : capture d'écran). S déjà enfoncé (état d'avant
        // cette frappe) : répétition d'un appui antérieur à Win, que l'appli a reçu ; avaler son relâchement
        // le laisserait enfoncé.
        if state != IDLE && win_down() && !modifiers_down() && !is_down(VK_S) {
            S_EATEN.store(true, Relaxed);
            enter(CHORD, state);
            mask_start_menu();
            request(SEARCH);
            return true;
        }
    }
    if physical && is_win(vk) {
        let held = HELD.load(Relaxed);
        if up {
            if vk == held {
                win_released(vk, state);
            }
        } else if state == IDLE {
            win_pressed(vk);
        } else if vk != held {
            enter(CHORD, state); // l'autre touche Win ; même touche = répétition automatique
        }
        return false;
    }
    if !up && (state == ALONE || state == MASK) {
        enter(CHORD, state);
    }
    false
}

fn win_pressed(vk: u32) {
    HELD.store(vk, Relaxed);
    // Ctrl, Maj ou Alt déjà tenue : raccourci du système (Ctrl+Win+D, touche Office...), laissé à Windows.
    if modifiers_down() {
        STATE.store(CHORD, Relaxed);
    } else {
        STATE.store(ALONE, Relaxed);
        post(MSG_MOUSE, 1);
    }
}

fn win_released(vk: u32, state: u8) {
    HELD.store(0, Relaxed);
    enter(IDLE, state);
    // L'état asynchrone n'est mis à jour qu'après les hooks : Win y paraît encore enfoncée, sauf si un hook
    // placé après le nôtre a avalé l'appui (bureau à distance, machine virtuelle). Il ne visait pas Windows.
    if !is_down(VIRTUAL_KEY(vk as u16)) {
        return;
    }
    if state == ALONE || state == MASK {
        mask_start_menu();
    }
    if state == ALONE {
        request(TOGGLE);
    }
}

fn enter(next: u8, prev: u8) {
    STATE.store(next, Relaxed);
    if prev == ALONE {
        post(MSG_MOUSE, 0);
    }
}

/// Glisse un appui factice avant le relâchement de Win, qui n'ouvre alors plus le menu Démarrer
/// (technique de PowerToys Keyboard Manager, licence MIT). Envoyé depuis le callback, il est traité avant
/// l'événement en cours ; SendInput rappelle notre hook au passage, d'où la signature TAG.
fn mask_start_menu() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: DUMMY, dwFlags: flags, dwExtraInfo: TAG, ..Default::default() } },
    };
    unsafe {
        SendInput(&[key(KEYBD_EVENT_FLAGS::default()), key(KEYEVENTF_KEYUP)], size_of::<INPUT>() as i32);
    }
}

fn request(mode: u8) {
    REQUEST.store(mode, Relaxed);
    if let Some(worker) = WORKER.get() {
        worker.unpark();
    }
}

fn is_win(vk: u32) -> bool {
    vk == VK_LWIN.0 as u32 || vk == VK_RWIN.0 as u32
}

/// État asynchrone du clavier : lecture rapide et sans verrou, juste même si des frappes nous ont échappé.
fn is_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) < 0 }
}

fn win_down() -> bool {
    is_down(VK_LWIN) || is_down(VK_RWIN)
}

fn modifiers_down() -> bool {
    is_down(VK_SHIFT) || is_down(VK_CONTROL) || is_down(VK_MENU)
}
