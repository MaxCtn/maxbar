//! Volume de la sortie audio par defaut (Core Audio).

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::AppHandle;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

use crate::publish;

#[derive(Serialize, PartialEq)]
struct Volume {
    level: u32,
    muted: bool,
}

pub enum Action {
    Set(f32),
    Step(f32),
    ToggleMute,
}

static TX: OnceLock<Sender<Action>> = OnceLock::new();

pub fn send(action: Action) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(action);
    }
}

fn endpoint() -> Option<IAudioEndpointVolume> {
    unsafe {
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = devices.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
    }
}

fn apply(ep: &IAudioEndpointVolume, action: Action) {
    unsafe {
        let level = ep.GetMasterVolumeLevelScalar().unwrap_or(0.0);
        let target = match action {
            Action::Set(v) => v,
            Action::Step(d) => level + d,
            Action::ToggleMute => {
                let muted = ep.GetMute().map(|m| m.as_bool()).unwrap_or(false);
                let _ = ep.SetMute(!muted, std::ptr::null());
                return;
            }
        };
        let target = target.clamp(0.0, 1.0);
        let _ = ep.SetMasterVolumeLevelScalar(target, std::ptr::null());
        if target > 0.0 && ep.GetMute().map(|m| m.as_bool()).unwrap_or(false) {
            let _ = ep.SetMute(false, std::ptr::null());
        }
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
            // Le peripherique par defaut peut changer (casque branche) : on le relit regulierement.
            if ep.is_none() || fetched.elapsed() > Duration::from_secs(5) {
                ep = endpoint();
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
