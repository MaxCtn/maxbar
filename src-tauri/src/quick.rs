//! Paramètres rapides : radios (Wi-Fi, Bluetooth), luminosité des écrans, modes d'alimentation.

use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::time::Duration;

use serde::Serialize;
use windows::core::{BOOL, GUID};
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, GetMonitorBrightness,
    GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR, QueryDisplayConfig, SetMonitorBrightness,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_DEVICE_INFO_TYPE, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME, PHYSICAL_MONITOR, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HANDLE, HLOCAL, LPARAM, LUID, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
use windows::Win32::System::Power::{
    PowerEnumerate, PowerGetActiveScheme, PowerReadFriendlyName, PowerSetActiveScheme, ACCESS_SCHEME,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_K, VK_LWIN, VK_P, VK_S, VK_SHIFT};

use crate::network::{from_wide, off_thread};

// -------------------------------------------------------------------- radios

#[derive(Serialize)]
pub struct RadioInfo {
    /// "wifi", "bluetooth" ou "other"
    kind: &'static str,
    name: String,
    on: bool,
}

static ACCESS_ASKED: AtomicBool = AtomicBool::new(false);

fn kind_name(kind: RadioKind) -> &'static str {
    match kind {
        RadioKind::WiFi => "wifi",
        RadioKind::Bluetooth => "bluetooth",
        _ => "other",
    }
}

/// Radios utilisables (celles désactivées dans le Gestionnaire de périphériques sont écartées).
fn radios() -> Vec<Radio> {
    // Windows retient la réponse : une demande par session suffit.
    if !ACCESS_ASKED.swap(true, Relaxed) {
        let _ = Radio::RequestAccessAsync().and_then(|op| op.join());
    }
    let Ok(list) = Radio::GetRadiosAsync().and_then(|op| op.join()) else { return Vec::new() };
    list.into_iter().filter(|r| r.State().is_ok_and(|s| s != RadioState::Disabled)).collect()
}

#[tauri::command]
pub async fn quick_radios() -> Vec<RadioInfo> {
    off_thread(|| {
        radios()
            .iter()
            .map(|r| RadioInfo {
                kind: r.Kind().map(kind_name).unwrap_or("other"),
                name: r.Name().map(|n| n.to_string()).unwrap_or_default(),
                on: r.State().is_ok_and(|s| s == RadioState::On),
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub async fn quick_set_radio(kind: String, on: bool) -> Result<(), String> {
    off_thread(move || {
        let state = if on { RadioState::On } else { RadioState::Off };
        let targets: Vec<Radio> =
            radios().into_iter().filter(|r| r.Kind().is_ok_and(|k| kind_name(k) == kind)).collect();
        if targets.is_empty() {
            return Err("Aucune radio de ce type sur cet ordinateur.".to_string());
        }
        for radio in targets {
            match radio.SetStateAsync(state).and_then(|op| op.join()) {
                Ok(RadioAccessStatus::Allowed) => {}
                Ok(RadioAccessStatus::DeniedByUser) => {
                    return Err("Accès aux radios refusé dans les paramètres de confidentialité.".into())
                }
                Ok(_) => return Err("Windows a refusé de changer l'état de la radio.".into()),
                Err(e) => return Err(format!("Impossible de changer l'état de la radio : {}", e.message())),
            }
        }
        Ok(())
    })
    .await
    .unwrap_or_else(|| Err("Opération interrompue.".into()))
}

// ------------------------------------------------------- luminosité DDC/CI

#[derive(Serialize, Clone)]
pub struct Monitor {
    id: String,
    name: String,
    /// Ramenée sur 0-100, quelle que soit la plage annoncée par l'écran.
    brightness: u32,
    supported: bool,
}

enum Job {
    List(Sender<Vec<Monitor>>),
    Set(String, u32),
}

/// Inactivité après laquelle les handles DDC/CI sont rendus à Windows.
const IDLE: Duration = Duration::from_secs(30);

static DDC: OnceLock<Sender<Job>> = OnceLock::new();

/// Fil DDC/CI, démarré à la première demande : un échange avec un écran prend des dizaines de
/// millisecondes et peut échouer, il ne doit bloquer ni l'interface ni le pool de Tauri.
fn ddc() -> &'static Sender<Job> {
    DDC.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        // Si le fil ne démarre pas, `rx` disparaît : les demandes échouent sans bruit.
        let _ = std::thread::Builder::new().name("ddc".into()).spawn(move || ddc_worker(rx));
        tx
    })
}

#[tauri::command]
pub async fn quick_monitors() -> Vec<Monitor> {
    let (tx, rx) = mpsc::channel();
    if ddc().send(Job::List(tx)).is_err() {
        return Vec::new();
    }
    tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(10)).unwrap_or_default())
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub fn quick_set_brightness(id: String, value: u32) {
    let _ = ddc().send(Job::Set(id, value.min(100)));
}

fn ddc_worker(rx: Receiver<Job>) {
    let mut screens: Option<Screens> = None;
    loop {
        let job = if screens.is_some() {
            match rx.recv_timeout(IDLE) {
                Ok(job) => job,
                Err(RecvTimeoutError::Timeout) => {
                    screens = None;
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(job) => job,
                Err(_) => return,
            }
        };

        // Pendant le glissement d'un curseur, seule la dernière valeur de chaque écran compte.
        let mut lists = Vec::new();
        let mut sets: Vec<(String, u32)> = Vec::new();
        for job in std::iter::once(job).chain(rx.try_iter()) {
            match job {
                Job::List(reply) => lists.push(reply),
                Job::Set(id, value) => {
                    sets.retain(|(other, _)| *other != id);
                    sets.push((id, value));
                }
            }
        }
        // Une liste demandée relit tout : un écran a pu être branché ou débranché entre-temps.
        if !lists.is_empty() {
            screens = None;
        }
        let opened = screens.get_or_insert_with(Screens::open);
        for (id, value) in sets {
            opened.set(&id, value);
        }
        if !lists.is_empty() {
            let snapshot = opened.snapshot();
            for reply in lists {
                let _ = reply.send(snapshot.clone());
            }
        }
    }
}

struct Screen {
    id: String,
    name: String,
    handle: HANDLE,
    /// Plage DDC/CI (min, max), absente si l'écran ne répond pas.
    range: Option<(u32, u32)>,
    level: u32,
}

/// Écrans physiques ouverts ; leurs handles sont rendus à la destruction.
struct Screens {
    list: Vec<Screen>,
    arrays: Vec<Vec<PHYSICAL_MONITOR>>,
}

impl Drop for Screens {
    fn drop(&mut self) {
        for array in &self.arrays {
            unsafe {
                let _ = DestroyPhysicalMonitors(array);
            }
        }
    }
}

unsafe extern "system" fn collect_monitor(hm: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
    if let Some(list) = (lp.0 as *mut Vec<HMONITOR>).as_mut() {
        list.push(hm);
    }
    TRUE
}

impl Screens {
    fn open() -> Self {
        let names = friendly_names();
        let mut monitors: Vec<HMONITOR> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(collect_monitor), LPARAM(&mut monitors as *mut _ as isize));
        }
        let mut screens = Screens { list: Vec::new(), arrays: Vec::new() };
        for hm in monitors {
            let device = gdi_device(hm);
            let mut count = 0u32;
            if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hm, &mut count) }.is_err() || count == 0 {
                continue;
            }
            let mut array = vec![PHYSICAL_MONITOR::default(); count as usize];
            if unsafe { GetPhysicalMonitorsFromHMONITOR(hm, &mut array) }.is_err() {
                continue;
            }
            for (i, physical) in array.iter().enumerate() {
                // Structure compactée : on copie les champs au lieu de les emprunter.
                let handle = physical.hPhysicalMonitor;
                let description = physical.szPhysicalMonitorDescription;
                let name = names
                    .get(&device)
                    .and_then(|n| n.get(i))
                    .cloned()
                    .or_else(|| Some(from_wide(&description)).filter(|d| !d.is_empty()))
                    .unwrap_or_else(|| format!("Écran {}", screens.list.len() + 1));
                let (range, level) = match read_brightness(handle) {
                    Some((min, current, max)) => (Some((min, max)), to_percent(current, min, max)),
                    None => (None, 0),
                };
                screens.list.push(Screen { id: format!("{device}/{i}"), name, handle, range, level });
            }
            screens.arrays.push(array);
        }
        screens
    }

    fn set(&mut self, id: &str, value: u32) {
        let Some(screen) = self.list.iter_mut().find(|s| s.id == id) else { return };
        let Some((min, max)) = screen.range else { return };
        let raw = min as u64 + ((max - min) as u64 * value as u64 + 50) / 100;
        if unsafe { SetMonitorBrightness(screen.handle, raw as u32) } != 0 {
            screen.level = value;
        }
    }

    fn snapshot(&self) -> Vec<Monitor> {
        self.list
            .iter()
            .map(|s| Monitor {
                id: s.id.clone(),
                name: s.name.clone(),
                brightness: s.level,
                supported: s.range.is_some(),
            })
            .collect()
    }
}

fn to_percent(current: u32, min: u32, max: u32) -> u32 {
    ((current.clamp(min, max) - min) as u64 * 100 / (max - min) as u64) as u32
}

/// (min, actuelle, max), ou None si l'écran ne parle pas DDC/CI.
fn read_brightness(handle: HANDLE) -> Option<(u32, u32, u32)> {
    // Certains écrans ignorent la toute première requête après l'ouverture du handle.
    for attempt in 0..2 {
        let (mut min, mut current, mut max) = (0u32, 0u32, 0u32);
        if unsafe { GetMonitorBrightness(handle, &mut min, &mut current, &mut max) } != 0 && max > min {
            return Some((min, current, max));
        }
        if attempt == 0 {
            std::thread::sleep(Duration::from_millis(60));
        }
    }
    None
}

/// Nom GDI du moniteur logique (\\.\DISPLAY1).
fn gdi_device(hm: HMONITOR) -> String {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    if unsafe { GetMonitorInfoW(hm, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) }.as_bool() {
        from_wide(&info.szDevice)
    } else {
        String::new()
    }
}

/// Noms commerciaux des écrans (« 27E6C ») par nom GDI : la description DDC/CI se contente
/// souvent de « Moniteur PnP générique ».
fn friendly_names() -> HashMap<String, Vec<String>> {
    let mut names: HashMap<String, Vec<String>> = HashMap::new();
    let (mut path_count, mut mode_count) = (0u32, 0u32);
    unsafe {
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count).is_err() {
            return names;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        let queried = QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        );
        if queried.is_err() {
            return names;
        }
        paths.truncate(path_count as usize);
        for path in &paths {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                header: info_header::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(
                    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                    path.sourceInfo.adapterId,
                    path.sourceInfo.id,
                ),
                ..Default::default()
            };
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
                header: info_header::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(
                    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                    path.targetInfo.adapterId,
                    path.targetInfo.id,
                ),
                ..Default::default()
            };
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0
                || DisplayConfigGetDeviceInfo(&mut target.header) != 0
            {
                continue;
            }
            let name = from_wide(&target.monitorFriendlyDeviceName);
            if !name.is_empty() {
                names.entry(from_wide(&source.viewGdiDeviceName)).or_default().push(name);
            }
        }
    }
    names
}

fn info_header<T>(kind: DISPLAYCONFIG_DEVICE_INFO_TYPE, adapter: LUID, id: u32) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER { r#type: kind, size: size_of::<T>() as u32, adapterId: adapter, id }
}

// ------------------------------------------------------- modes d'alimentation

#[derive(Serialize)]
pub struct PowerPlan {
    guid: String,
    name: String,
    active: bool,
}

#[tauri::command]
pub async fn quick_power_plans() -> Vec<PowerPlan> {
    tauri::async_runtime::spawn_blocking(power_plans).await.unwrap_or_default()
}

fn power_plans() -> Vec<PowerPlan> {
    let active = active_scheme();
    let mut plans = Vec::new();
    for index in 0..32 {
        let mut guid = GUID::zeroed();
        let mut size = size_of::<GUID>() as u32;
        let r = unsafe {
            PowerEnumerate(None, None, None, ACCESS_SCHEME, index, Some(&mut guid as *mut GUID as *mut u8), &mut size)
        };
        if r != ERROR_SUCCESS {
            break;
        }
        plans.push(PowerPlan {
            guid: format!("{guid:?}"),
            name: plan_name(&guid).unwrap_or_else(|| "Mode personnalisé".into()),
            active: active == Some(guid),
        });
    }
    plans
}

fn plan_name(guid: &GUID) -> Option<String> {
    let mut size = 0u32;
    unsafe {
        if PowerReadFriendlyName(None, Some(guid as *const GUID), None, None, None, &mut size) != ERROR_SUCCESS
            || size == 0
        {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        let r = PowerReadFriendlyName(
            None,
            Some(guid as *const GUID),
            None,
            None,
            Some(buf.as_mut_ptr() as *mut u8),
            &mut size,
        );
        if r != ERROR_SUCCESS {
            return None;
        }
        Some(from_wide(&buf)).filter(|n| !n.is_empty())
    }
}

fn active_scheme() -> Option<GUID> {
    let mut p: *mut GUID = std::ptr::null_mut();
    unsafe {
        if PowerGetActiveScheme(None, &mut p) != ERROR_SUCCESS || p.is_null() {
            return None;
        }
        let guid = *p;
        let _ = LocalFree(Some(HLOCAL(p as *mut c_void)));
        Some(guid)
    }
}

#[tauri::command]
pub async fn quick_set_power_plan(guid: String) -> Result<(), String> {
    let guid = GUID::try_from(guid.as_str()).map_err(|_| "Mode d'alimentation inconnu.".to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        match unsafe { PowerSetActiveScheme(None, Some(&guid as *const GUID)) } {
            ERROR_SUCCESS => Ok(()),
            _ => Err("Windows a refusé de changer de mode d'alimentation.".to_string()),
        }
    })
    .await
    .unwrap_or_else(|_| Err("Opération interrompue.".into()))
}

// ----------------------------------------------------------------- raccourcis

/// Volets du shell sans équivalent maxbar : projection, cast, capture d'écran.
#[tauri::command]
pub fn quick_shortcut(name: String) {
    let keys: &[VIRTUAL_KEY] = match name.as_str() {
        "project" => &[VK_LWIN, VK_P],
        "cast" => &[VK_LWIN, VK_K],
        "snip" => &[VK_LWIN, VK_SHIFT, VK_S],
        _ => return,
    };
    crate::sys::press(keys);
}
