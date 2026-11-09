//! Météo via Open-Meteo (gratuit, sans clé). Position : ville de la config, sinon géolocalisation IP.

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::{publish, ConfigState};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const EVERY: Duration = Duration::from_secs(15 * 60);
const RETRY: Duration = Duration::from_secs(2 * 60);

static TX: OnceLock<Sender<()>> = OnceLock::new();

#[derive(Clone)]
struct Place {
    lat: f64,
    lon: f64,
    name: String,
}

/// Relance une mise à jour (ex. ville modifiée dans la config).
pub fn poke() {
    if let Some(tx) = TX.get() {
        let _ = tx.send(());
    }
}

pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    let _ = TX.set(tx);
    std::thread::spawn(move || {
        let mut place: Option<(String, Place)> = None;
        loop {
            while crate::appbar::fullscreen_active() {
                std::thread::sleep(Duration::from_secs(5));
            }
            let city = app.state::<ConfigState>().0.lock().unwrap().weather_city.trim().to_string();
            if place.as_ref().map(|p| &p.0) != Some(&city) {
                place = locate(&city).map(|p| (city.clone(), p));
            }
            let data = place.as_ref().and_then(|(_, p)| forecast(p).map(|d| (p.name.clone(), d)));
            let ok = data.is_some();
            if let Some((name, data)) = data {
                publish(&app, "weather", &json!({ "city": name, "data": data }));
            }
            match rx.recv_timeout(if ok { EVERY } else { RETRY }) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}

/// Requête via curl.exe (fourni avec Windows) : le client HTTP WinRT échoue en TLS sur Open-Meteo.
fn get(url: &str) -> Option<Value> {
    let out = Command::new("curl.exe")
        .args(["-sSL", "--max-time", "15", "-A", "maxbar/0.2", url])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match out {
        Ok(o) if o.status.success() => serde_json::from_slice(&o.stdout).ok(),
        other => {
            let why = match other {
                Ok(o) => String::from_utf8_lossy(&o.stderr).to_string(),
                Err(e) => e.to_string(),
            };
            let _ = std::fs::write(crate::config::path().with_file_name("weather.log"), format!("{url}\n{why}\n"));
            None
        }
    }
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse().ok())
}

fn locate(city: &str) -> Option<Place> {
    if !city.is_empty() {
        let r = get(&format!(
            "https://geocoding-api.open-meteo.com/v1/search?count=1&language=fr&format=json&name={}",
            encode(city)
        ))?;
        let first = r.get("results")?.get(0)?;
        return Some(Place {
            lat: num(first.get("latitude")?)?,
            lon: num(first.get("longitude")?)?,
            name: first.get("name").and_then(Value::as_str).unwrap_or(city).to_string(),
        });
    }
    let ip = |url: &str, lat: &str, lon: &str| -> Option<Place> {
        let r = get(url)?;
        Some(Place {
            lat: num(r.get(lat)?)?,
            lon: num(r.get(lon)?)?,
            name: r.get("city").and_then(Value::as_str).unwrap_or_default().to_string(),
        })
    };
    ip("https://get.geojs.io/v1/ip/geo.json", "latitude", "longitude")
        .or_else(|| ip("http://ip-api.com/json/?fields=lat,lon,city", "lat", "lon"))
}

fn forecast(p: &Place) -> Option<Value> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={:.3}&longitude={:.3}&timezone=auto&forecast_days=6&forecast_hours=25\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,is_day,wind_speed_10m\
         &hourly=temperature_2m,weather_code,precipitation_probability,is_day\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,sunrise,sunset",
        p.lat, p.lon
    );
    get(&url).filter(|v| v.get("current").is_some())
}
