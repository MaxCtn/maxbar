#![windows_subsystem = "windows"]

mod appbar;
mod audio;
mod claude;
mod config;
mod icons;
mod media;
mod stats;
mod sys;
mod tasks;
mod tray;
mod weather;

use std::sync::Mutex;

use config::Config;
use serde_json::{Map, Value};
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, Wry};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_A, VK_CONTROL, VK_D, VK_ESCAPE, VK_LWIN, VK_N, VK_S, VK_SHIFT, VK_X,
};

/// Dernier etat publie par chaque source, pour que l'interface demarre avec des donnees.
#[derive(Default)]
pub struct Store(Mutex<Map<String, Value>>);

pub struct ConfigState(pub Mutex<Config>);

struct TrayItems {
    top: CheckMenuItem<Wry>,
    bottom: CheckMenuItem<Wry>,
    hide_taskbar: CheckMenuItem<Wry>,
    remove_taskbar: CheckMenuItem<Wry>,
    autostart: CheckMenuItem<Wry>,
}

/// Contexte du dernier menu clic-droit ouvert sur une icone.
struct TaskMenu {
    path: String,
    hwnds: Vec<isize>,
}
static TASK_MENU: Mutex<Option<TaskMenu>> = Mutex::new(None);

/// Publie un etat vers l'interface, seulement s'il a change.
pub fn publish<T: serde::Serialize>(app: &AppHandle, topic: &str, payload: &T) {
    let Ok(value) = serde_json::to_value(payload) else { return };
    if let Some(store) = app.try_state::<Store>() {
        let mut map = store.0.lock().unwrap();
        if map.get(topic) == Some(&value) {
            return;
        }
        map.insert(topic.to_string(), value.clone());
    }
    let _ = app.emit(topic, value);
}

fn current_config(app: &AppHandle) -> Config {
    app.state::<ConfigState>().0.lock().unwrap().clone()
}

// ------------------------------------------------------------------ commandes

#[tauri::command]
fn snapshot(store: tauri::State<Store>) -> Map<String, Value> {
    store.0.lock().unwrap().clone()
}

#[tauri::command]
fn activate(hwnd: isize) {
    tasks::activate(hwnd);
}

#[tauri::command]
fn launch(path: String) {
    if !path.is_empty() {
        sys::open(&path);
    }
}

#[tauri::command]
fn media_control(action: String) {
    media::control(&action);
}

/// Zones visibles et cliquables en plus de la barre (ilot, panneaux, volet).
#[tauri::command]
fn set_shapes(shapes: Vec<appbar::Shape>) {
    appbar::set_shapes(shapes);
}

#[tauri::command]
fn tray_click(id: String, button: String) {
    tray::click(&id, &button);
}

#[tauri::command]
fn volume(action: String, value: Option<f32>) {
    match action.as_str() {
        "set" => audio::send(audio::Action::Set(value.unwrap_or(0.0))),
        "step" => audio::send(audio::Action::Step(value.unwrap_or(0.0))),
        "mute" => audio::send(audio::Action::ToggleMute),
        _ => {}
    }
}

/// Raccourcis du shell Windows (menu Demarrer, notifications, parametres rapides...).
#[tauri::command]
fn shortcut(name: String) {
    let keys: &[VIRTUAL_KEY] = match name.as_str() {
        "start" => &[VK_LWIN],
        "power" => &[VK_LWIN, VK_X],
        "search" => &[VK_LWIN, VK_S],
        "notifications" => &[VK_LWIN, VK_N],
        "quick" => &[VK_LWIN, VK_A],
        "desktop" => &[VK_LWIN, VK_D],
        "taskmgr" => &[VK_CONTROL, VK_SHIFT, VK_ESCAPE],
        "settings" => return sys::open("ms-settings:"),
        "sound" => return sys::open("ms-settings:sound"),
        "network" => return sys::open("ms-settings:network"),
        "date" => return sys::open("ms-settings:dateandtime"),
        _ => return,
    };
    sys::press(keys);
}

#[tauri::command]
fn task_menu(app: AppHandle, window: WebviewWindow, path: String, hwnds: Vec<isize>, titles: Vec<String>) -> Result<(), String> {
    let menu = build_task_menu(&app, &path, &hwnds, &titles).map_err(|e| e.to_string())?;
    *TASK_MENU.lock().unwrap() = Some(TaskMenu { path, hwnds });
    window.popup_menu(&menu).map_err(|e| e.to_string())
}

fn build_task_menu(app: &AppHandle, path: &str, hwnds: &[isize], titles: &[String]) -> tauri::Result<Menu<Wry>> {
    let pinned = current_config(app).pinned.iter().any(|p| p.eq_ignore_ascii_case(path));
    let menu = Menu::new(app)?;
    for (h, t) in hwnds.iter().zip(titles).take(12) {
        let mut label: String = t.chars().take(60).collect();
        if t.chars().count() > 60 {
            label.push('…');
        }
        menu.append(&MenuItem::with_id(app, format!("act:{h}"), label.replace('&', "&&"), true, None::<&str>)?)?;
    }
    if !hwnds.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    if !path.is_empty() {
        let open = if hwnds.is_empty() { "Ouvrir" } else { "Nouvelle fenêtre" };
        menu.append(&MenuItem::with_id(app, "ctx:new", open, true, None::<&str>)?)?;
        let pin = if pinned { "Désépingler" } else { "Épingler à la barre" };
        menu.append(&MenuItem::with_id(app, "ctx:pin", pin, true, None::<&str>)?)?;
    }
    if !hwnds.is_empty() {
        let close = if hwnds.len() > 1 { "Fermer toutes les fenêtres" } else { "Fermer la fenêtre" };
        menu.append(&MenuItem::with_id(app, "ctx:close", close, true, None::<&str>)?)?;
    }
    Ok(menu)
}

// ---------------------------------------------------------------------- menus

fn apply_config(app: &AppHandle, cfg: Config) {
    *app.state::<ConfigState>().0.lock().unwrap() = cfg.clone();
    appbar::configure(cfg.bottom(), cfg.height);
    if cfg.hide_windows_taskbar != appbar::taskbar_hidden() {
        appbar::set_taskbar_hidden(cfg.hide_windows_taskbar);
    }
    publish(app, "config", &cfg);
    sync_tray(app);
    tasks::poke();
    weather::poke();
}

fn update_config(app: &AppHandle, change: impl FnOnce(&mut Config)) {
    let mut cfg = current_config(app);
    change(&mut cfg);
    config::save(&cfg);
    apply_config(app, cfg);
}

fn sync_tray(app: &AppHandle) {
    let Some(t) = app.try_state::<TrayItems>() else { return };
    let bottom = current_config(app).bottom();
    let _ = t.top.set_checked(!bottom);
    let _ = t.bottom.set_checked(bottom);
    let _ = t.hide_taskbar.set_checked(appbar::taskbar_autohide());
    let _ = t.remove_taskbar.set_checked(appbar::taskbar_hidden());
    let _ = t.autostart.set_checked(sys::autostart_enabled());
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "quit" => {
            appbar::remove();
            app.exit(0);
        }
        "config" => sys::open(&config::path().to_string_lossy()),
        "reload" => apply_config(app, config::load()),
        "edge-top" | "edge-bottom" => update_config(app, |c| c.edge = if id == "edge-top" { "top" } else { "bottom" }.into()),
        "hide-taskbar" => {
            appbar::set_taskbar_autohide(!appbar::taskbar_autohide());
            sync_tray(app);
        }
        "remove-taskbar" => update_config(app, |c| c.hide_windows_taskbar = !c.hide_windows_taskbar),
        "import-pins" => update_config(app, |c| {
            let pins = config::windows_pins();
            if !pins.is_empty() {
                c.pinned = pins;
            }
        }),
        "autostart" => {
            sys::set_autostart(!sys::autostart_enabled());
            sync_tray(app);
        }
        "ctx:new" | "ctx:pin" | "ctx:close" => {
            let Some(ctx) = TASK_MENU.lock().unwrap().take() else { return };
            match id {
                "ctx:new" => sys::open(&ctx.path),
                "ctx:close" => ctx.hwnds.iter().for_each(|&h| tasks::close(h)),
                _ => update_config(app, |c| {
                    let before = c.pinned.len();
                    c.pinned.retain(|p| !p.eq_ignore_ascii_case(&ctx.path));
                    if c.pinned.len() == before {
                        c.pinned.push(ctx.path);
                    }
                }),
            }
        }
        other => {
            if let Some(h) = other.strip_prefix("act:").and_then(|s| s.parse().ok()) {
                tasks::activate(h);
            }
        }
    }
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let cfg = current_config(app.handle());
    let top = CheckMenuItem::with_id(app, "edge-top", "Barre en haut de l'écran", true, !cfg.bottom(), None::<&str>)?;
    let bottom = CheckMenuItem::with_id(app, "edge-bottom", "Barre en bas de l'écran", true, cfg.bottom(), None::<&str>)?;
    let hide_taskbar = CheckMenuItem::with_id(
        app,
        "hide-taskbar",
        "Masquer automatiquement la barre des tâches Windows",
        true,
        appbar::taskbar_autohide(),
        None::<&str>,
    )?;
    let remove_taskbar = CheckMenuItem::with_id(
        app,
        "remove-taskbar",
        "Cacher complètement la barre des tâches Windows",
        true,
        cfg.hide_windows_taskbar,
        None::<&str>,
    )?;
    let import = MenuItem::with_id(app, "import-pins", "Réimporter les épingles de Windows", true, None::<&str>)?;
    let autostart =
        CheckMenuItem::with_id(app, "autostart", "Lancer au démarrage de Windows", true, sys::autostart_enabled(), None::<&str>)?;
    let edit = MenuItem::with_id(app, "config", "Modifier la configuration…", true, None::<&str>)?;
    let reload = MenuItem::with_id(app, "reload", "Recharger la configuration", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quitter maxbar", true, None::<&str>)?;
    let (s1, s2, s3) =
        (PredefinedMenuItem::separator(app)?, PredefinedMenuItem::separator(app)?, PredefinedMenuItem::separator(app)?);
    let items: [&dyn IsMenuItem<Wry>; 12] =
        [&top, &bottom, &s1, &hide_taskbar, &remove_taskbar, &import, &autostart, &s2, &edit, &reload, &s3, &quit];
    let menu = Menu::with_items(app, &items)?;

    let mut tray = TrayIconBuilder::with_id("maxbar").tooltip("maxbar").menu(&menu).show_menu_on_left_click(true);
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(TrayItems { top, bottom, hide_taskbar, remove_taskbar, autostart });
    Ok(())
}

/// Demande a WebView2 de garder une empreinte memoire minimale (caches reduits).
fn low_memory(win: &WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW};
    use windows::core::Interface;
    let _ = win.with_webview(|wv| unsafe {
        if let Ok(core) = wv.controller().CoreWebView2() {
            if let Ok(core19) = core.cast::<ICoreWebView2_19>() {
                let _ = core19.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
            }
        }
    });
}

fn main() {
    if !sys::single_instance() {
        return;
    }
    // Ne jamais laisser une bande d'ecran reservee (ni la barre Windows cachee) si l'app plante.
    std::panic::set_hook(Box::new(|_| appbar::remove()));

    let cfg = config::load();
    tauri::Builder::default()
        .manage(Store::default())
        .manage(ConfigState(Mutex::new(cfg.clone())))
        .invoke_handler(tauri::generate_handler![
            snapshot,
            activate,
            launch,
            media_control,
            set_shapes,
            tray_click,
            volume,
            shortcut,
            task_menu
        ])
        // Alt+F4 sur la barre ne doit pas la fermer : on quitte via l'icone de notification.
        .on_window_event(|_, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            publish(&handle, "config", &cfg);
            appbar::init(&handle, cfg.bottom(), cfg.height)?;
            if cfg.hide_windows_taskbar {
                appbar::set_taskbar_hidden(true);
            }
            build_tray(app)?;
            app.on_menu_event(|app, event| on_menu(app, event.id().as_ref()));
            tasks::start(handle.clone());
            media::start(handle.clone());
            stats::start(handle.clone());
            audio::start(handle.clone());
            weather::start(handle.clone());
            if cfg.tray {
                tray::start(handle.clone());
            }
            claude::start(handle);
            if let Some(win) = app.get_webview_window("bar") {
                low_memory(&win);
            }
            appbar::show();
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("maxbar : demarrage impossible")
        .run(|_, event| {
            if let tauri::RunEvent::Exit = event {
                appbar::remove();
            }
        });
}
