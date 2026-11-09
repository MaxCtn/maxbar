//! Son (Core Audio) : volume de la sortie par défaut, choix de la sortie, volume par application.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::AppHandle;
use windows::core::{Interface, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::S_OK;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eMultimedia, eRender, AudioSessionStateExpired, IAudioSessionControl2,
    IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};

use crate::{icons, publish, sys, tasks};

#[derive(Serialize, PartialEq)]
struct Volume {
    level: u32,
    muted: bool,
}

#[derive(Serialize)]
pub struct Device {
    id: String,
    name: String,
    default: bool,
}

/// Une application qui joue du son (ses sessions sont regroupées par exe).
#[derive(Serialize)]
pub struct App {
    key: String,
    name: String,
    icon: Option<String>,
    level: u32,
    muted: bool,
}

#[derive(Serialize)]
pub struct Mixer {
    devices: Vec<Device>,
    apps: Vec<App>,
}

pub enum Action {
    Set(f32),
    Step(f32),
    ToggleMute,
    /// La sortie par défaut a changé : relire le périphérique.
    Reload,
}

static TX: OnceLock<Sender<Action>> = OnceLock::new();
static ICONS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

const SYSTEM_SOUNDS: &str = "system";

pub fn send(action: Action) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(action);
    }
}

pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<Action>();
    let _ = TX.set(tx);
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut ep: Option<IAudioEndpointVolume> = None;
        let mut fetched = Instant::now();
        loop {
            if crate::appbar::fullscreen_active() {
                match rx.recv_timeout(Duration::from_secs(2)) {
                    Err(RecvTimeoutError::Disconnected) => return,
                    _ => continue,
                }
            }
            // La sortie par défaut peut changer (casque branché) : on la relit régulièrement.
            if ep.is_none() || fetched.elapsed() > Duration::from_secs(5) {
                ep = endpoint_volume();
                fetched = Instant::now();
            }
            let state = ep.as_ref().and_then(|e| unsafe {
                Some(Volume {
                    level: (e.GetMasterVolumeLevelScalar().ok()? * 100.0).round() as u32,
                    muted: e.GetMute().ok()?.as_bool(),
                })
            });
            if state.is_none() {
                ep = None;
            }
            publish(&app, "volume", &state);
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(Action::Reload) => ep = None,
                Ok(action) => {
                    if let Some(e) = &ep {
                        apply(e, action);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}

fn enumerator() -> Option<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
}

fn default_device() -> Option<IMMDevice> {
    unsafe { enumerator()?.GetDefaultAudioEndpoint(eRender, eConsole).ok() }
}

fn endpoint_volume() -> Option<IAudioEndpointVolume> {
    unsafe { default_device()?.Activate(CLSCTX_ALL, None).ok() }
}

fn apply(ep: &IAudioEndpointVolume, action: Action) {
    unsafe {
        let muted = ep.GetMute().map(|m| m.as_bool()).unwrap_or(false);
        let level = ep.GetMasterVolumeLevelScalar().unwrap_or(0.0);
        let target = match action {
            Action::Set(v) => v,
            Action::Step(d) => level + d,
            Action::ToggleMute => {
                let _ = ep.SetMute(!muted, std::ptr::null());
                return;
            }
            Action::Reload => return,
        };
        let target = target.clamp(0.0, 1.0);
        let _ = ep.SetMasterVolumeLevelScalar(target, std::ptr::null());
        if target > 0.0 && muted {
            let _ = ep.SetMute(false, std::ptr::null());
        }
    }
}

/// Exécute `f` sur un thread COM dédié (les commandes Tauri n'ont pas d'appartement COM garanti).
fn with_com<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        f()
    })
    .join()
    .expect("thread audio")
}

// ------------------------------------------------------------- sorties audio

pub fn mixer() -> Mixer {
    with_com(|| Mixer { devices: devices(), apps: default_device().map(|d| apps(&d)).unwrap_or_default() })
}

fn device_id(d: &IMMDevice) -> Option<String> {
    unsafe {
        let raw = d.GetId().ok()?;
        let id = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const c_void));
        id
    }
}

fn device_name(d: &IMMDevice) -> Option<String> {
    unsafe {
        let store = d.OpenPropertyStore(STGM_READ).ok()?;
        let name = store.GetValue(&PKEY_Device_FriendlyName).ok()?.to_string();
        (!name.is_empty()).then_some(name)
    }
}

fn devices() -> Vec<Device> {
    let Some(en) = enumerator() else { return Vec::new() };
    let default = default_device().and_then(|d| device_id(&d));
    let mut out = Vec::new();
    unsafe {
        let Ok(list) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return out };
        for i in 0..list.GetCount().unwrap_or(0) {
            let Ok(d) = list.Item(i) else { continue };
            let Some(id) = device_id(&d) else { continue };
            out.push(Device {
                name: device_name(&d).unwrap_or_else(|| "Sortie audio".into()),
                default: default.as_ref() == Some(&id),
                id,
            });
        }
    }
    out.sort_by_key(|d| d.name.to_lowercase());
    out
}

/// Interface non documentée de Windows, utilisée par le panneau Son lui-même
/// (et par EarTrumpet, SoundSwitch...) pour changer la sortie par défaut.
/// Seul `SetDefaultEndpoint` sert, mais l'ordre de la vtable impose de déclarer les précédentes.
#[allow(non_snake_case, dead_code)]
mod policy {
    use std::ffi::c_void;
    use windows::core::{GUID, HRESULT, PCWSTR};
    use windows::Win32::Media::Audio::ERole;
    use windows_core::{interface, IUnknown, IUnknown_Vtbl};

    pub const CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: IUnknown {
        fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
        fn GetDeviceFormat(&self, device: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
        fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
        fn SetDeviceFormat(&self, device: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
        fn GetProcessingPeriod(&self, device: PCWSTR, default: i32, period: *mut i64, min: *mut i64) -> HRESULT;
        fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
        fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn GetPropertyValue(&self, device: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
        fn SetPropertyValue(&self, device: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
        fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
        fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
    }

    /// Les méthodes générées par #[interface] sont privées au module.
    pub unsafe fn set_default(policy: &IPolicyConfig, device: PCWSTR, role: ERole) -> HRESULT {
        policy.SetDefaultEndpoint(device, role)
    }
}

pub fn set_output(id: String) {
    with_com(move || unsafe {
        let created = CoCreateInstance::<_, policy::IPolicyConfig>(&policy::CLIENT, None, CLSCTX_ALL);
        let Ok(policy) = created else { return };
        let wide = sys::wide(&id);
        for role in [eConsole, eMultimedia, eCommunications] {
            let _ = policy::set_default(&policy, PCWSTR(wide.as_ptr()), role);
        }
    });
    send(Action::Reload);
}

// ------------------------------------------------------- volume par application

/// Parcourt les sessions audio actives de la sortie par défaut, avec la clé de leur application.
fn for_each_session(device: &IMMDevice, mut f: impl FnMut(String, &ISimpleAudioVolume)) {
    unsafe {
        let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return };
        let Ok(sessions) = manager.GetSessionEnumerator() else { return };
        for i in 0..sessions.GetCount().unwrap_or(0) {
            let Ok(control) = sessions.GetSession(i) else { continue };
            if control.GetState().map_or(true, |s| s == AudioSessionStateExpired) {
                continue;
            }
            let (Ok(control), Ok(volume)) =
                (control.cast::<IAudioSessionControl2>(), control.cast::<ISimpleAudioVolume>())
            else {
                continue;
            };
            let key = if control.IsSystemSoundsSession() == S_OK {
                SYSTEM_SOUNDS.to_string()
            } else {
                tasks::exe_path(control.GetProcessId().unwrap_or(0)).to_lowercase()
            };
            if !key.is_empty() {
                f(key, &volume);
            }
        }
    }
}

fn apps(device: &IMMDevice) -> Vec<App> {
    let mut by_key: Vec<App> = Vec::new();
    for_each_session(device, |key, volume| unsafe {
        let level = (volume.GetMasterVolume().unwrap_or(0.0) * 100.0).round() as u32;
        let muted = volume.GetMute().map(|m| m.as_bool()).unwrap_or(false);
        if let Some(app) = by_key.iter_mut().find(|a| a.key == key) {
            app.level = app.level.max(level);
            app.muted &= muted;
            return;
        }
        let (name, icon) = describe(&key);
        by_key.push(App { key, name, icon, level, muted });
    });
    // Sons système en dernier, le reste par nom.
    by_key.sort_by_key(|a| (a.key == SYSTEM_SOUNDS, a.name.to_lowercase()));
    by_key
}

fn describe(key: &str) -> (String, Option<String>) {
    if key == SYSTEM_SOUNDS {
        return ("Sons système".into(), None);
    }
    let name = sys::file_description(key).unwrap_or_else(|| {
        std::path::Path::new(key).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    });
    let mut cache = ICONS.lock().unwrap();
    let icon = cache.get_or_insert_with(HashMap::new).entry(key.to_string()).or_insert_with(|| icons::from_path(key));
    (name, icon.clone())
}

/// Règle le volume (0..1) ou bascule la sourdine d'une application.
pub fn set_app(key: String, level: Option<f32>, toggle_mute: bool) {
    with_com(move || {
        let Some(device) = default_device() else { return };
        let mut mute: Option<bool> = None;
        for_each_session(&device, |k, volume| unsafe {
            if k != key {
                return;
            }
            if let Some(level) = level {
                let _ = volume.SetMasterVolume(level.clamp(0.0, 1.0), std::ptr::null());
                let _ = volume.SetMute(false, std::ptr::null());
            }
            if toggle_mute {
                // Toutes les sessions de l'app prennent l'état inverse de la première.
                let m = *mute.get_or_insert_with(|| !volume.GetMute().map(|m| m.as_bool()).unwrap_or(false));
                let _ = volume.SetMute(m, std::ptr::null());
            }
        });
    });
}
