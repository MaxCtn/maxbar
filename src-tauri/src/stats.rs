//! CPU, RAM, GPU, reseau, batterie - echantillonnes toutes les 2 s.

use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::AppHandle;
use windows::core::w;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W,
    PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

use crate::publish;

const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;
const IF_TYPE_ETHERNET: u32 = 6;
const IF_TYPE_WIFI: u32 = 71;
const PDH_MORE_DATA: u32 = 0x800007D2;

#[derive(Serialize, PartialEq)]
struct Battery {
    percent: u8,
    charging: bool,
}

#[derive(Serialize, PartialEq)]
struct Stats {
    cpu: f32,
    ram: f32,
    ram_used_gb: f64,
    ram_total_gb: f64,
    gpu: Option<f32>,
    net_down: u64,
    net_up: u64,
    /// "ethernet", "wifi" ou "none"
    net: &'static str,
    battery: Option<Battery>,
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut cpu_prev = cpu_times();
        let mut net_prev = (net_bytes(), Instant::now());
        let gpu = Gpu::open();
        std::thread::sleep(Duration::from_millis(500));
        loop {
            // En jeu plein ecran : aucune mesure (les compteurs GPU peuvent provoquer des saccades).
            if crate::appbar::fullscreen_active() {
                std::thread::sleep(Duration::from_secs(2));
                cpu_prev = None;
                continue;
            }
            let cpu_now = cpu_times();
            let cpu = match (cpu_prev, cpu_now) {
                (Some((i0, t0)), Some((i1, t1))) if t1 > t0 => (1.0 - (i1 - i0) as f64 / (t1 - t0) as f64) * 100.0,
                _ => 0.0,
            };
            cpu_prev = cpu_now;

            let ((down, up, net), at) = (net_bytes(), Instant::now());
            let secs = at.duration_since(net_prev.1).as_secs_f64().max(0.001);
            let rate = |a: u64, b: u64| (b.saturating_sub(a) as f64 / secs) as u64;
            let (net_down, net_up) = (rate(net_prev.0 .0, down), rate(net_prev.0 .1, up));
            net_prev = ((down, up, net), at);

            let (ram, used, total) = memory();
            publish(
                &app,
                "stats",
                &Stats {
                    cpu: cpu.clamp(0.0, 100.0).round() as f32,
                    ram,
                    ram_used_gb: used,
                    ram_total_gb: total,
                    gpu: gpu.as_ref().and_then(Gpu::read).map(|g| g.round() as f32),
                    net_down,
                    net_up,
                    net,
                    battery: battery(),
                },
            );
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

/// (idle, total) ; total = kernel (inclut idle) + user.
fn cpu_times() -> Option<(u64, u64)> {
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).ok()? };
    Some((ft(idle), ft(kernel) + ft(user)))
}

fn memory() -> (f32, f64, f64) {
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut m) }.is_err() {
        return (0.0, 0.0, 0.0);
    }
    let gb = |b: u64| (b as f64 / 1_073_741_824.0 * 10.0).round() / 10.0;
    (m.dwMemoryLoad as f32, gb(m.ullTotalPhys - m.ullAvailPhys), gb(m.ullTotalPhys))
}

/// Octets recus/emis cumules sur les interfaces physiques actives (hors filtres et loopback),
/// et type de connexion (filaire prioritaire sur le wifi).
fn net_bytes() -> (u64, u64, &'static str) {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    let (mut down, mut up) = (0u64, 0u64);
    let (mut ethernet, mut wifi) = (false, false);
    unsafe {
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return (0, 0, "none");
        }
        let t = &*table;
        let rows = std::slice::from_raw_parts(t.Table.as_ptr(), t.NumEntries as usize);
        for r in rows {
            let flags = r.InterfaceAndOperStatusFlags._bitfield;
            let hardware = flags & 0b001 != 0;
            let filter = flags & 0b010 != 0;
            if r.Type == IF_TYPE_SOFTWARE_LOOPBACK || r.OperStatus != IfOperStatusUp || !hardware || filter {
                continue;
            }
            down += r.InOctets;
            up += r.OutOctets;
            ethernet |= r.Type == IF_TYPE_ETHERNET;
            wifi |= r.Type == IF_TYPE_WIFI;
        }
        FreeMibTable(table as *const _);
    }
    (down, up, if ethernet { "ethernet" } else if wifi { "wifi" } else { "none" })
}

fn battery() -> Option<Battery> {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).ok()? };
    if s.BatteryFlag & 128 != 0 || s.BatteryLifePercent > 100 {
        return None;
    }
    Some(Battery { percent: s.BatteryLifePercent, charging: s.ACLineStatus == 1 })
}

/// Utilisation 3D du GPU via les compteurs de performance (meme source que le Gestionnaire des taches).
struct Gpu {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
}

impl Gpu {
    fn open() -> Option<Self> {
        unsafe {
            let mut query = PDH_HQUERY::default();
            if PdhOpenQueryW(None, 0, &mut query) != 0 {
                return None;
            }
            let mut counter = PDH_HCOUNTER::default();
            if PdhAddEnglishCounterW(query, w!("\\GPU Engine(*engtype_3D)\\Utilization Percentage"), 0, &mut counter) != 0 {
                return None;
            }
            PdhCollectQueryData(query);
            Some(Self { query, counter })
        }
    }

    fn read(&self) -> Option<f64> {
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            let (mut size, mut count) = (0u32, 0u32);
            if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, None) != PDH_MORE_DATA {
                return Some(0.0);
            }
            // u64 pour garantir l'alignement des structures retournees.
            let mut buf = vec![0u64; size as usize / 8 + 1];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
                return None;
            }
            let items = std::slice::from_raw_parts(items, count as usize);
            let total: f64 = items.iter().filter(|i| i.FmtValue.CStatus == 0).map(|i| i.FmtValue.Anonymous.doubleValue).sum();
            Some(total.clamp(0.0, 100.0))
        }
    }
}
