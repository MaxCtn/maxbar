//! Configuration utilisateur : %APPDATA%\maxbar\config.json

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Config {
    /// "top" ou "bottom"
    pub edge: String,
    /// Hauteur reservee en pixels logiques (DPI-independants).
    pub height: u32,
    /// Apps epinglees : exe, raccourci .lnk ou URL.
    pub pinned: Vec<String>,
    pub show_music: bool,
    pub show_claude: bool,
    pub show_system: bool,
    pub show_weather: bool,
    pub clock_seconds: bool,
    /// Ville pour la meteo ; vide = position approximative via l'adresse IP.
    pub weather_city: String,
    /// Opacite du fond de la barre (0 = transparente, 1 = opaque).
    pub opacity: f64,
    /// Icones de la zone de notification relayees dans maxbar.
    pub tray: bool,
    /// Barre des taches Windows completement cachee (et non simplement en masquage auto).
    pub hide_windows_taskbar: bool,
    /// Les epingles de la barre Windows ont deja ete importees (une seule fois).
    pub windows_pins_imported: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            edge: "top".into(),
            height: 42,
            pinned: default_pins(),
            show_music: true,
            show_claude: true,
            show_system: true,
            show_weather: true,
            clock_seconds: false,
            weather_city: String::new(),
            opacity: 0.72,
            tray: true,
            hide_windows_taskbar: false,
            windows_pins_imported: false,
        }
    }
}

impl Config {
    pub fn bottom(&self) -> bool {
        self.edge == "bottom"
    }
}

fn default_pins() -> Vec<String> {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let candidates = [
        r"C:\Windows\explorer.exe".to_string(),
        format!(r"{}\Google\Chrome\Application\chrome.exe", env("ProgramFiles")),
        format!(r"{}\Microsoft\Edge\Application\msedge.exe", env("ProgramFiles(x86)")),
        format!(r"{}\Programs\Microsoft VS Code\Code.exe", env("LOCALAPPDATA")),
        format!(r"{}\Steam\steam.exe", env("ProgramFiles(x86)")),
    ];
    let mut pins: Vec<String> = candidates.into_iter().filter(|p| Path::new(p).exists()).collect();
    // Un seul navigateur par defaut.
    if pins.iter().any(|p| p.ends_with("chrome.exe")) {
        pins.retain(|p| !p.ends_with("msedge.exe"));
    }
    pins
}

/// Raccourcis epingles a la barre des taches Windows.
pub fn windows_pins() -> Vec<String> {
    let dir = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_default().join(
        r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar",
    );
    let mut pins: Vec<String> = std::fs::read_dir(dir)
        .map(|it| {
            it.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")))
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    // L'explorateur de fichiers en premier, comme sur la barre Windows.
    pins.sort_by_key(|p| {
        let name = Path::new(p).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        (!name.contains("explor"), name)
    });
    pins
}

pub fn path() -> PathBuf {
    let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    base.join("maxbar").join("config.json")
}

pub fn load() -> Config {
    let p = path();
    let mut c = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<Config>(&s).ok()).unwrap_or_default();
    c.height = c.height.clamp(28, 96);
    c.opacity = c.opacity.clamp(0.0, 1.0);
    if !c.windows_pins_imported {
        let pins = windows_pins();
        if !pins.is_empty() {
            c.pinned = pins;
        }
        c.windows_pins_imported = true;
    }
    save(&c);
    c
}

pub fn save(c: &Config) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string_pretty(c) {
        let _ = std::fs::write(p, s);
    }
}
