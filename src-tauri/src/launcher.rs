//! Lanceur : menu Démarrer et recherche (applications, réglages, fichiers récents, alimentation).
//!
//! L'index reprend exactement « Toutes les applications » du menu Démarrer : le dossier shell
//! AppsFolder, qui réunit apps empaquetées (identifiées par leur AUMID) et raccourcis de bureau.
//! Il est gardé en cache disque pour que le lanceur soit complet dès l'ouverture de session.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use windows::core::{w, Error, Interface, GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GlobalFree, HANDLE, LUID, PROPERTYKEY, SIZE};
use windows::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, SE_SHUTDOWN_NAME,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::Storage::FileSystem::GetDriveTypeW;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, STGM_READ,
};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};
use windows::Win32::System::Shutdown::{
    ExitWindowsEx, LockWorkStation, EWX_HYBRID_SHUTDOWN, EWX_LOGOFF, EWX_POWEROFF, EWX_REBOOT, EWX_SHUTDOWN,
    SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OTHER, SHTDN_REASON_MINOR_OTHER,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItem2, IShellItemImageFactory, IShellLinkW,
    SHGetKnownFolderItem, SHGetKnownFolderPath, ShellExecuteExW, ShellLink, KF_FLAG_DEFAULT, SEE_MASK_NOASYNC,
    SHELLEXECUTEINFOW, SIGDN, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::{appbar, config, icons, sys};

const ICON_SIZE: i32 = 48;
/// L'ouverture de session a mieux à faire que de lire 300 icônes.
const STARTUP_DELAY: Duration = Duration::from_secs(4);
/// Âge au-delà duquel l'ouverture du lanceur relance une indexation en arrière-plan.
const STALE_AFTER: Duration = Duration::from_secs(600);
const CACHE_FORMAT: u32 = 1;
const RECENT_MAX: usize = 12;
/// Raccourcis récents examinés au plus (cibles supprimées, doublons...).
const RECENT_SCAN: usize = 60;
const USAGE_MAX: usize = 400;
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_REMOTE: u32 = 4;
const DRIVE_CDROM: u32 = 5;
/// La liste des fichiers récents est relue au plus tôt après ce délai, si le dossier Recent n'a pas changé.
const RECENT_TTL: Duration = Duration::from_secs(30);

/// System.Link.TargetParsingPath (propkey.h), redéclarée pour ne pas activer Win32_Storage_EnhancedStorage.
const PKEY_LINK_TARGET_PARSING_PATH: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0xb9b4b3fc_2b51_4a42_b5d8_324146afcf25), pid: 2 };

/// Liens d'aide, de documentation ou de désinstallation que les installeurs déposent dans le menu Démarrer.
const NOISE_NAMES: [&str; 21] = [
    "uninstall",
    "désinstall",
    "desinstall",
    "deinstall",
    "readme",
    "read me",
    "lisez-moi",
    "lisezmoi",
    "documentation",
    "release notes",
    "notes de version",
    "website",
    "web site",
    "site web",
    "site internet",
    "user manual",
    "user guide",
    "manuel d'utilisation",
    "guide de l'utilisateur",
    "online help",
    "aide en ligne",
];
const DOC_EXTENSIONS: [&str; 14] = [
    ".chm", ".hlp", ".htm", ".html", ".mht", ".url", ".pdf", ".txt", ".rtf", ".md", ".xps", ".doc", ".docx", ".website",
];
/// Types dont chaque fichier a sa propre icône (les autres partagent celle de leur extension).
const OWN_ICON: [&str; 10] = ["exe", "lnk", "ico", "url", "msc", "cpl", "scr", "appref-ms", "cur", "ani"];

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct App {
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
}

struct Index {
    apps: Vec<App>,
    version: u64,
    /// Dernière indexation complète de cette session (None : seulement le cache disque).
    built: Option<Instant>,
}

#[derive(Serialize)]
struct CacheOut<'a> {
    format: u32,
    apps: &'a [App],
}

#[derive(Deserialize)]
struct CacheIn {
    format: u32,
    apps: Vec<App>,
}

/// Usage et épingles du lanceur (%APPDATA%\maxbar\launcher.json).
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct State {
    usage: HashMap<String, Usage>,
    pins: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Default)]
#[serde(default)]
struct Usage {
    count: u32,
    /// Dernier lancement, en ms depuis l'époque Unix.
    last: u64,
}

#[derive(Serialize, Clone)]
pub struct Recent {
    name: String,
    path: String,
    /// Dossier parent, relatif au profil utilisateur quand il s'y trouve.
    folder: String,
    modified_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
}

static INDEX: Mutex<Index> = Mutex::new(Index { apps: Vec::new(), version: 0, built: None });
static WAKE: OnceLock<Sender<()>> = OnceLock::new();
/// Indexation demandée ou en cours : les demandes suivantes sont ignorées.
static PENDING: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<Option<State>> = Mutex::new(None);
/// Derniers fichiers récents lus : date de modification du dossier Recent, heure de lecture, liste.
static RECENT: Mutex<Option<(SystemTime, Instant, Vec<Recent>)>> = Mutex::new(None);
static FILE_ICONS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// COM initialisé pour la durée de vie de la valeur (appartement STA, comme l'attend le shell).
struct Com(bool);

impl Com {
    fn init() -> Self {
        Com(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

/// Exécute `f` sur un thread du pool bloquant avec COM initialisé, hors du thread principal.
async fn in_shell<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _com = Com::init();
        f()
    })
    .await
    .map_err(|e| e.to_string())
}

fn now_ms() -> u64 {
    ms(SystemTime::now())
}

fn ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn data_file(name: &str) -> PathBuf {
    config::path().with_file_name(name)
}

/// Écrit dans un fichier temporaire puis le renomme : jamais de fichier à moitié écrit.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    // Sur le disque avant le renommage : après une coupure, on retrouve l'ancien contenu ou le nouveau.
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)
}

// ------------------------------------------------------------------ indexation

pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel();
    let _ = WAKE.set(tx);
    let _ = std::thread::Builder::new().name("launcher-index".into()).spawn(move || indexer(&app, &rx));
}

fn indexer(app: &AppHandle, rx: &Receiver<()>) {
    let _com = Com::init();
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
    if let Some(apps) = load_cache() {
        install(app, apps, false);
    }
    // Sauf si le lanceur est ouvert entre-temps (launcher_refresh réveille le thread).
    if let Err(RecvTimeoutError::Disconnected) = rx.recv_timeout(STARTUP_DELAY) {
        return;
    }
    let mut first = true;
    loop {
        wait_while_fullscreen();
        PENDING.store(true, Relaxed);
        // La première indexation de la session relit toutes les icônes (apps mises à jour), les suivantes
        // reprennent celles déjà connues.
        let reuse: HashMap<String, String> = if first {
            HashMap::new()
        } else {
            lock(&INDEX).apps.iter().filter_map(|a| Some((a.id.clone(), a.icon.clone()?))).collect()
        };
        if let Some(apps) = scan(&reuse) {
            install(app, apps, true);
        }
        first = false;
        PENDING.store(false, Relaxed);
        if rx.recv().is_err() {
            return;
        }
    }
}

fn wait_while_fullscreen() {
    while appbar::fullscreen_anywhere() {
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn install(app: &AppHandle, apps: Vec<App>, fresh: bool) {
    let (version, cache) = {
        let mut index = lock(&INDEX);
        if fresh {
            index.built = Some(Instant::now());
        }
        if index.apps == apps {
            return;
        }
        let cache = if fresh { serde_json::to_vec(&CacheOut { format: CACHE_FORMAT, apps: &apps }).ok() } else { None };
        index.apps = apps;
        index.version += 1;
        (index.version, cache)
    };
    if let Some(bytes) = cache {
        let _ = write_atomic(&data_file("apps.json"), &bytes);
    }
    let _ = app.emit("launcher-index", version);
}

fn load_cache() -> Option<Vec<App>> {
    let text = std::fs::read_to_string(data_file("apps.json")).ok()?;
    let cache: CacheIn = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    (cache.format == CACHE_FORMAT && !cache.apps.is_empty()).then_some(cache.apps)
}

fn scan(reuse: &HashMap<String, String>) -> Option<Vec<App>> {
    unsafe {
        let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None).ok()?;
        let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems).ok()?;
        let mut apps = Vec::new();
        let mut seen = HashSet::new();
        loop {
            let mut slot = [None];
            if items.Next(&mut slot, None).is_err() {
                break;
            }
            let [Some(item)] = slot else { break };
            wait_while_fullscreen();
            let Some(app) = describe(&item, reuse) else { continue };
            let target = app.path.as_deref().unwrap_or(&app.id).to_lowercase();
            if seen.insert((app.name.to_lowercase(), target)) {
                apps.push(app);
            }
        }
        // Une énumération vide est un échec du shell, pas un menu Démarrer vide.
        if apps.is_empty() {
            return None;
        }
        apps.sort_by_cached_key(|a| a.name.to_lowercase());
        Some(apps)
    }
}

unsafe fn describe(item: &IShellItem, reuse: &HashMap<String, String>) -> Option<App> {
    let name = display_name(item, SIGDN_NORMALDISPLAY)?.trim().to_string();
    // Relatif à AppsFolder : l'AUMID d'une app empaquetée, ou l'identifiant d'une app de bureau.
    let id = display_name(item, SIGDN_PARENTRELATIVEPARSING)?;
    if name.is_empty() || id.is_empty() {
        return None;
    }
    let path = target(item, &id);
    if is_noise(&name, &id, path.as_deref()) {
        return None;
    }
    let icon = reuse.get(&id).cloned().or_else(|| image(item));
    Some(App { id, name, path, icon })
}

unsafe fn take_string(p: PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = p.to_string().ok();
    CoTaskMemFree(Some(p.0 as *const c_void));
    s
}

unsafe fn display_name(item: &IShellItem, kind: SIGDN) -> Option<String> {
    take_string(item.GetDisplayName(kind).ok()?)
}

/// Fichier visé par une app de bureau (None pour une app empaquetée ou un emplacement virtuel).
unsafe fn target(item: &IShellItem, id: &str) -> Option<String> {
    let linked = item
        .cast::<IShellItem2>()
        .ok()
        .and_then(|item| take_string(item.GetString(&PKEY_LINK_TARGET_PARSING_PATH).ok()?))
        .filter(|p| !p.is_empty());
    let path = linked.or_else(|| known_folder_path(id))?;
    let p = Path::new(&path);
    (p.is_absolute() && p.exists()).then_some(path)
}

/// Les apps de bureau sans AUMID ont pour identifiant leur chemin, dossier connu en tête :
/// « {6D809377-6AF0-444B-8957-A3773F02200E}\7-Zip\7zFM.exe ».
unsafe fn known_folder_path(id: &str) -> Option<String> {
    if id.as_bytes().get(1) == Some(&b':') {
        return Some(id.to_string());
    }
    let (guid, tail) = id.strip_prefix('{')?.split_once('}')?;
    let guid = GUID::try_from(guid).ok()?;
    let base = take_string(SHGetKnownFolderPath(&guid, KF_FLAG_DEFAULT, None).ok()?)?;
    Some(format!("{}{tail}", base.trim_end_matches('\\')))
}

fn is_noise(name: &str, id: &str, path: Option<&str>) -> bool {
    let name = name.to_lowercase();
    let lower_id = id.to_lowercase();
    if ["http:", "https:", "www."].iter().any(|p| lower_id.starts_with(p)) {
        return true;
    }
    if NOISE_NAMES.iter().any(|w| name.contains(w)) {
        return true;
    }
    [Some(id), path].into_iter().flatten().any(|s| {
        let file = Path::new(s).file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
        file.starts_with("unins") || file.contains("uninstall") || DOC_EXTENSIONS.iter().any(|e| file.ends_with(e))
    })
}

unsafe fn image(item: &IShellItem) -> Option<String> {
    let factory: IShellItemImageFactory = item.cast().ok()?;
    let bmp = factory.GetImage(SIZE { cx: ICON_SIZE, cy: ICON_SIZE }, SIIGBF_ICONONLY).ok()?;
    let png = icons::from_bitmap(bmp);
    let _ = DeleteObject(HGDIOBJ(bmp.0));
    png
}

// ------------------------------------------------------------- applications

#[tauri::command]
pub async fn launcher_apps() -> Vec<App> {
    lock(&INDEX).apps.clone()
}

/// Appelée à chaque ouverture du lanceur : réindexe en arrière-plan si l'index a vieilli.
#[tauri::command]
pub fn launcher_refresh() {
    let stale = lock(&INDEX).built.is_none_or(|t| t.elapsed() > STALE_AFTER);
    if stale && !PENDING.swap(true, Relaxed) {
        if let Some(tx) = WAKE.get() {
            let _ = tx.send(());
        }
    }
}

/// Chemin cible d'une app de l'index ; Err si l'identifiant est inconnu.
fn app_path(id: &str) -> Result<Option<String>, String> {
    lock(&INDEX)
        .apps
        .iter()
        .find(|a| a.id == id)
        .map(|a| a.path.clone())
        .ok_or_else(|| "application inconnue".to_string())
}

fn execute(verb: &str, file: &str, params: Option<&str>) -> Result<(), String> {
    let verb = sys::wide(verb);
    let file = sys::wide(file);
    let params = params.map(sys::wide);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // Le thread du pool peut libérer COM juste après : on attend la fin de l'activation.
        fMask: SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: params.as_ref().map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr())),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| e.message())
}

#[tauri::command]
pub async fn launcher_launch(id: String, admin: bool) -> Result<(), String> {
    let path = app_path(&id)?;
    record_use(&id);
    let uri = format!("shell:AppsFolder\\{id}");
    // « Exécuter en tant qu'administrateur » vise l'exe lui-même quand on le connaît.
    let (verb, file) = if admin { ("runas", path.unwrap_or(uri)) } else { ("open", uri) };
    in_shell(move || execute(verb, &file, None)).await?
}

/// Ouvre l'Explorateur sur l'exe d'une app, ou directement sur un fichier existant (fichiers récents).
#[tauri::command]
pub async fn launcher_reveal(id: String) -> Result<(), String> {
    let path = match app_path(&id) {
        Ok(path) => path.ok_or_else(|| "emplacement inconnu".to_string())?,
        Err(_) if Path::new(&id).is_absolute() && Path::new(&id).exists() => id,
        Err(e) => return Err(e),
    };
    let params = format!("/select,\"{path}\"");
    in_shell(move || execute("open", "explorer.exe", Some(&params))).await?
}

// ------------------------------------------------------------ usage, épingles

fn load_state() -> State {
    let p = data_file("launcher.json");
    let Ok(text) = std::fs::read_to_string(&p) else { return State::default() };
    match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(state) => state,
        Err(_) => {
            // Fichier illisible : gardé de côté plutôt qu'écrasé.
            let _ = std::fs::copy(&p, p.with_extension("json.bak"));
            State::default()
        }
    }
}

/// Applique `f` à l'état (lu à la première utilisation), puis l'enregistre si `save`.
fn with_state<R>(save: bool, f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = lock(&STATE);
    let state = guard.get_or_insert_with(load_state);
    let out = f(state);
    if save {
        if let Ok(bytes) = serde_json::to_vec_pretty(state) {
            let _ = write_atomic(&data_file("launcher.json"), &bytes);
        }
    }
    out
}

fn record_use(id: &str) {
    let now = now_ms();
    with_state(true, |s| {
        let usage = s.usage.entry(id.to_string()).or_default();
        usage.count = usage.count.saturating_add(1);
        usage.last = now;
        let excess = s.usage.len().saturating_sub(USAGE_MAX);
        if excess > 0 {
            let mut by_age: Vec<(u64, String)> = s.usage.iter().map(|(k, u)| (u.last, k.clone())).collect();
            by_age.sort_unstable();
            for (_, k) in by_age.into_iter().take(excess) {
                s.usage.remove(&k);
            }
        }
    });
}

#[tauri::command]
pub async fn launcher_state() -> State {
    with_state(false, |s| s.clone())
}

#[tauri::command]
pub async fn launcher_pin(id: String, pinned: bool) -> State {
    with_state(true, |s| {
        if !pinned {
            s.pins.retain(|p| p != &id);
        } else if !s.pins.contains(&id) {
            s.pins.push(id);
        }
        s.clone()
    })
}

// ----------------------------------------------------------- fichiers récents

#[tauri::command]
pub async fn launcher_recent() -> Vec<Recent> {
    in_shell(recent_files).await.unwrap_or_default()
}

/// Relue à chaque ouverture du lanceur : le résultat est gardé tant que le dossier Recent ne change pas.
fn recent_files() -> Vec<Recent> {
    let Some(appdata) = std::env::var_os("APPDATA") else { return Vec::new() };
    let dir = Path::new(&appdata).join(r"Microsoft\Windows\Recent");
    let stamp = std::fs::metadata(&dir).and_then(|m| m.modified()).ok();
    if let Some((at, read, list)) = lock(&RECENT).as_ref() {
        if stamp == Some(*at) && read.elapsed() < RECENT_TTL {
            return list.clone();
        }
    }
    let list = read_recent(&dir);
    if let Some(stamp) = stamp {
        *lock(&RECENT) = Some((stamp, Instant::now(), list.clone()));
    }
    list
}

fn read_recent(dir: &Path) -> Vec<Recent> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut links: Vec<(u64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if !path.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                return None;
            }
            Some((ms(e.metadata().ok()?.modified().ok()?), path))
        })
        .collect();
    links.sort_unstable_by_key(|l| Reverse(l.0));

    let profile = std::env::var("USERPROFILE").unwrap_or_default();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (modified_ms, lnk) in links.into_iter().take(RECENT_SCAN) {
        if out.len() == RECENT_MAX {
            break;
        }
        let Some(target) = link_target(&lnk) else { continue };
        if !seen.insert(target.to_lowercase()) {
            continue;
        }
        // Un partage hors ligne ou un lecteur à réveiller bloquerait de longues secondes : on ne le touche pas.
        let slow = is_slow_drive(&target);
        if !slow && !Path::new(&target).is_file() {
            continue;
        }
        let p = Path::new(&target);
        let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        let folder = p.parent().map(|d| pretty_folder(d, &profile)).unwrap_or_default();
        let icon = file_icon(&target, slow);
        out.push(Recent { name, path: target, folder, modified_ms, icon });
    }
    out
}

fn link_target(lnk: &Path) -> Option<String> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        let w = sys::wide(&lnk.to_string_lossy());
        file.Load(PCWSTR(w.as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
        let target = String::from_utf16_lossy(&buf[..n]);
        (!target.is_empty()).then_some(target)
    }
}

/// Partage réseau, clé USB, lecteur optique.
fn is_slow_drive(path: &str) -> bool {
    if path.starts_with(r"\\") {
        return true;
    }
    let Some(root) = path.get(..3) else { return false };
    let w = sys::wide(root);
    let kind = unsafe { GetDriveTypeW(PCWSTR(w.as_ptr())) };
    matches!(kind, DRIVE_REMOTE | DRIVE_REMOVABLE | DRIVE_CDROM)
}

fn pretty_folder(dir: &Path, profile: &str) -> String {
    let d = dir.to_string_lossy();
    let rest = d.get(..profile.len()).filter(|head| !profile.is_empty() && head.eq_ignore_ascii_case(profile));
    if rest.is_some() {
        if let Some(rest) = d.get(profile.len()..) {
            if rest.is_empty() {
                return "Dossier personnel".into();
            }
            if let Some(rest) = rest.strip_prefix('\\') {
                return rest.to_string();
            }
        }
    }
    d.into_owned()
}

/// Les documents partagent l'icône de leur type : une seule extraction par extension.
fn file_icon(path: &str, slow: bool) -> Option<String> {
    let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let key = if OWN_ICON.contains(&ext.as_str()) { path.to_lowercase() } else { format!(".{ext}") };
    if let Some(icon) = lock(&FILE_ICONS).get_or_insert_with(HashMap::new).get(&key) {
        return icon.clone();
    }
    if slow {
        return None;
    }
    // Extraction hors du verrou : une autre lecture n'a pas à l'attendre.
    let icon = icons::from_path(path);
    lock(&FILE_ICONS).get_or_insert_with(HashMap::new).insert(key, icon.clone());
    icon
}

/// Recherche de l'Explorateur dans le profil utilisateur.
#[tauri::command]
pub async fn launcher_search_files(query: String) -> Result<(), String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("recherche vide".into());
    }
    let profile = std::env::var("USERPROFILE").map_err(|e| e.to_string())?;
    let uri = format!("search-ms:query={}&crumb=location:{}", encode(query), encode(&profile));
    in_shell(move || execute("open", &uri, None)).await?
}

/// Encodage pourcent UTF-8 de tout sauf les caractères non réservés (RFC 3986).
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

// ------------------------------------------------------------ presse-papiers

#[tauri::command]
pub async fn clipboard_set(text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || copy_text(&text)).await.map_err(|e| e.to_string())?
}

fn copy_text(text: &str) -> Result<(), String> {
    let wide = sys::wide(text);
    let owner = Some(appbar::hwnd()).filter(|h| !h.is_invalid());
    unsafe {
        // Une autre app peut tenir le presse-papiers quelques millisecondes.
        let mut tries = 0;
        while let Err(e) = OpenClipboard(owner) {
            tries += 1;
            if tries == 6 {
                return Err(e.message());
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        let result = (|| {
            EmptyClipboard()?;
            let mem = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2)?;
            let dst = GlobalLock(mem) as *mut u16;
            if dst.is_null() {
                let err = Error::from_thread();
                let _ = GlobalFree(Some(mem));
                return Err(err);
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
            let _ = GlobalUnlock(mem);
            // En cas de succès, la mémoire appartient désormais au système.
            if let Err(e) = SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(mem.0))) {
                let _ = GlobalFree(Some(mem));
                return Err(e);
            }
            Ok(())
        })();
        let _ = CloseClipboard();
        result.map_err(|e| e.message())
    }
}

// --------------------------------------------------------------- alimentation

/// Verrouiller, mettre en veille, se déconnecter, redémarrer ou arrêter, comme le menu Démarrer.
#[tauri::command]
pub async fn power_action(action: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || power(&action)).await.map_err(|e| e.to_string())?
}

fn power(action: &str) -> Result<(), String> {
    let reason = SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_MINOR_OTHER | SHTDN_REASON_FLAG_PLANNED;
    unsafe {
        match action {
            "lock" => LockWorkStation().map_err(|e| e.message()),
            "sleep" => {
                enable_shutdown_privilege()?;
                // Peut ne rendre la main qu'au réveil : d'où le thread à part.
                if SetSuspendState(false, false, false) {
                    Ok(())
                } else {
                    Err(Error::from_thread().message())
                }
            }
            "logoff" | "restart" | "shutdown" => {
                enable_shutdown_privilege()?;
                let flags = match action {
                    "logoff" => EWX_LOGOFF,
                    "restart" => EWX_REBOOT,
                    _ => EWX_SHUTDOWN | EWX_POWEROFF,
                };
                // Comme le menu Démarrer : extinction hybride si le démarrage rapide est activé.
                let hybrid = action == "shutdown" && fast_startup();
                if hybrid && ExitWindowsEx(flags | EWX_HYBRID_SHUTDOWN, reason).is_ok() {
                    return Ok(());
                }
                ExitWindowsEx(flags, reason).map_err(|e| e.message())
            }
            _ => Err(format!("action inconnue : {action}")),
        }
    }
}

fn enable_shutdown_privilege() -> Result<(), String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token)
            .map_err(|e| e.message())?;
        let mut luid = LUID::default();
        let result = LookupPrivilegeValueW(PCWSTR::null(), SE_SHUTDOWN_NAME, &mut luid).and_then(|()| {
            let privileges = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
            };
            AdjustTokenPrivileges(token, false, Some(&privileges), 0, None, None)
        });
        let _ = CloseHandle(token);
        result.map_err(|e| e.message())
    }
}

/// Démarrage rapide (Hiberboot) activé dans les options d'alimentation.
fn fast_startup() -> bool {
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SYSTEM\CurrentControlSet\Control\Session Manager\Power"),
            w!("HiberbootEnabled"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    err.is_ok() && value == 1
}
