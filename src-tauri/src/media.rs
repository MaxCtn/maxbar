//! Lecture en cours via les contrôles média de Windows (Spotify, navigateurs, Deezer...).

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use tauri::AppHandle;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Props,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::{Buffer, DataReader, InputStreamOptions};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::publish;

/// Écart entre l'époque Windows (1601) et Unix, en unités de 100 ns.
const EPOCH_DIFF: i64 = 116_444_736_000_000_000;

#[derive(Serialize, PartialEq, Default)]
struct Media {
    title: String,
    artist: String,
    album: String,
    source: String,
    playing: bool,
    position_ms: i64,
    duration_ms: i64,
    updated_ms: i64,
    can_prev: bool,
    can_next: bool,
}

static TX: OnceLock<Sender<String>> = OnceLock::new();

pub fn control(action: &str) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(action.to_string());
    }
}

pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<String>();
    let _ = TX.set(tx);
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut manager: Option<Manager> = None;
        let mut track = String::new();
        let mut art_tries = 0u8;
        let mut has_art = false;
        loop {
            // Barre masquée (jeu plein écran) : pas d'interrogation des lecteurs.
            if crate::appbar::fullscreen_active() {
                if let Err(RecvTimeoutError::Disconnected) = rx.recv_timeout(Duration::from_secs(2)) {
                    return;
                }
                continue;
            }
            if manager.is_none() {
                manager = Manager::RequestAsync().and_then(|op| op.join()).ok();
            }
            let session = manager.as_ref().and_then(|m| m.GetCurrentSession().ok());
            let state = session.as_ref().and_then(|s| read(s).ok());
            if let (Some(s), Some(m)) = (&session, &state) {
                let key = format!("{}\u{1}{}\u{1}{}", m.title, m.artist, m.album);
                if key != track {
                    track = key;
                    art_tries = 0;
                    has_art = false;
                    publish(&app, "art", &Option::<String>::None);
                }
                // Spotify & co publient la pochette un peu après le titre : quelques essais.
                if !has_art && art_tries < 6 {
                    art_tries += 1;
                    if let Some(art) =
                        s.TryGetMediaPropertiesAsync().and_then(|op| op.join()).ok().and_then(|p| thumbnail(&p))
                    {
                        has_art = true;
                        publish(&app, "art", &Some(art));
                    }
                }
            }
            publish(&app, "media", &state);
            match rx.recv_timeout(Duration::from_millis(1000)) {
                Ok(action) => {
                    if let Some(s) = &session {
                        let _ = act(s, &action);
                    }
                    std::thread::sleep(Duration::from_millis(120));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}

fn read(s: &Session) -> windows::core::Result<Media> {
    let props = s.TryGetMediaPropertiesAsync()?.join()?;
    let playback = s.GetPlaybackInfo()?;
    let controls = playback.Controls()?;
    let timeline = s.GetTimelineProperties()?;
    let updated = timeline.LastUpdatedTime()?.UniversalTime;
    Ok(Media {
        title: props.Title()?.to_string(),
        artist: props.Artist()?.to_string(),
        album: props.AlbumTitle()?.to_string(),
        source: source_name(&s.SourceAppUserModelId()?.to_string()),
        playing: playback.PlaybackStatus()? == Status::Playing,
        position_ms: timeline.Position()?.Duration / 10_000,
        duration_ms: timeline.EndTime()?.Duration / 10_000,
        updated_ms: if updated > EPOCH_DIFF { (updated - EPOCH_DIFF) / 10_000 } else { 0 },
        can_prev: controls.IsPreviousEnabled().unwrap_or(false),
        can_next: controls.IsNextEnabled().unwrap_or(false),
    })
}

/// "Spotify.exe" -> "Spotify", "SpotifyAB.SpotifyMusic_xxx!Spotify" -> "Spotify".
fn source_name(aumid: &str) -> String {
    let s = aumid.rsplit('!').next().unwrap_or(aumid);
    let s = s.trim_end_matches(".exe").trim_end_matches(".EXE");
    match s.to_lowercase().as_str() {
        "msedge" => "Edge".into(),
        "chrome" => "Chrome".into(),
        "firefox" => "Firefox".into(),
        _ => {
            let mut c = s.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

fn thumbnail(p: &Props) -> Option<String> {
    let reference = p.Thumbnail().ok()?;
    let stream = reference.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 8_000_000 {
        return None;
    }
    let buffer = Buffer::Create(size).ok()?;
    let filled = stream.ReadAsync(&buffer, size, InputStreamOptions::None).ok()?.join().ok()?;
    let reader = DataReader::FromBuffer(&filled).ok()?;
    let mut bytes = vec![0u8; filled.Length().ok()? as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    let mime = stream.ContentType().map(|m| m.to_string()).unwrap_or_default();
    let mime = if mime.starts_with("image/") { mime } else { "image/png".into() };
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn act(s: &Session, action: &str) -> windows::core::Result<()> {
    match action {
        "toggle" => s.TryTogglePlayPauseAsync()?.join().map(|_| ()),
        "next" => s.TrySkipNextAsync()?.join().map(|_| ()),
        "prev" => s.TrySkipPreviousAsync()?.join().map(|_| ()),
        _ => Ok(()),
    }
}
