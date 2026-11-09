//! Utilisation de Claude Code : quota 5 h / 7 j (ecrit par la statusline de Claude Monitor
//! dans ~/.claude/monitor/quota.json), historique pour les deltas, sessions actives.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use tauri::AppHandle;

use crate::publish;

const ACTIVE_SECS: u64 = 120;
/// Historique conserve : de quoi calculer la conso du jour et de la derniere fenetre de 5 h.
const KEEP_SECS: i64 = 26 * 3600;
/// Valeur inchangee : un point toutes les 5 min suffit pour la courbe.
const IDLE_POINT_SECS: i64 = 300;

#[derive(Serialize, PartialEq)]
struct Window {
    used: f64,
    resets_at: Option<i64>,
}

#[derive(Serialize, PartialEq)]
struct Session {
    project: String,
    idle_s: u64,
}

#[derive(Serialize, PartialEq)]
struct Claude {
    five_hour: Option<Window>,
    seven_day: Option<Window>,
    updated: Option<f64>,
    sessions: Vec<Session>,
    /// [horodatage, % 5 h, % 7 j]
    history: Vec<[f64; 3]>,
}

fn home() -> PathBuf {
    std::env::var("USERPROFILE").map(PathBuf::from).unwrap_or_default().join(".claude")
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

struct History {
    rows: Vec<(i64, f64, f64)>,
    file: PathBuf,
    appended: usize,
}

impl History {
    fn load() -> Self {
        let file = crate::config::path().with_file_name("claude-history.csv");
        let mut rows: Vec<(i64, f64, f64)> = std::fs::read_to_string(&file)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let mut it = l.split(',');
                Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?))
            })
            .collect();
        // Historique de Claude Monitor, s'il existe : de quoi avoir des deltas des le premier lancement.
        let monitor = std::fs::read_to_string(home().join("monitor").join("history.jsonl")).unwrap_or_default();
        rows.extend(monitor.lines().filter_map(|l| {
            let v: Value = serde_json::from_str(l).ok()?;
            Some((v.get("t")?.as_f64()? as i64, v.get("q5")?.as_f64()?, v.get("q7")?.as_f64()?))
        }));
        rows.sort_by_key(|r| r.0);
        rows.dedup_by_key(|r| r.0);
        let mut h = Self { rows, file, appended: 0 };
        h.compact();
        h
    }

    /// Ne garde que les changements (plus un point par 5 min) sur 26 h, et reecrit le fichier.
    fn compact(&mut self) {
        let cutoff = now() - KEEP_SECS;
        let mut kept: Vec<(i64, f64, f64)> = Vec::with_capacity(self.rows.len());
        for &r in self.rows.iter().filter(|r| r.0 >= cutoff) {
            if let Some(&(t, a, b)) = kept.last() {
                if a == r.1 && b == r.2 && r.0 - t < IDLE_POINT_SECS {
                    continue;
                }
            }
            kept.push(r);
        }
        self.rows = kept;
        let text: String = self.rows.iter().map(|(t, a, b)| format!("{t},{a},{b}\n")).collect();
        let _ = std::fs::write(&self.file, text);
        self.appended = 0;
    }

    fn push(&mut self, t: i64, q5: f64, q7: f64) {
        if let Some(&(lt, a, b)) = self.rows.last() {
            if t <= lt || (a == q5 && b == q7 && t - lt < IDLE_POINT_SECS) {
                return;
            }
        }
        self.rows.push((t, q5, q7));
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.file) {
            let _ = writeln!(f, "{t},{q5},{q7}");
        }
        self.appended += 1;
        if self.appended > 400 {
            self.compact();
        }
    }
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut history = History::load();
        loop {
            let mut snap = snapshot();
            if let (Some(t), Some(f), Some(s)) = (snap.updated, &snap.five_hour, &snap.seven_day) {
                history.push(t as i64, f.used, s.used);
            }
            let cutoff = (now() - KEEP_SECS) as f64;
            snap.history =
                history.rows.iter().map(|&(t, a, b)| [t as f64, a, b]).filter(|r| r[0] >= cutoff).collect();
            publish(&app, "claude", &snap);
            std::thread::sleep(Duration::from_secs(10));
        }
    });
}

fn snapshot() -> Claude {
    let root = home();
    let quota: Option<Value> =
        std::fs::read_to_string(root.join("monitor").join("quota.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
    let window = |name: &str| {
        let w = quota.as_ref()?.get("rate_limits")?.get(name)?;
        Some(Window { used: w.get("used_percentage")?.as_f64()?, resets_at: w.get("resets_at").and_then(Value::as_i64) })
    };
    Claude {
        five_hour: window("five_hour"),
        seven_day: window("seven_day"),
        updated: quota.as_ref().and_then(|q| q.get("_ts")).and_then(Value::as_f64),
        sessions: sessions(&root.join("projects")),
        history: Vec::new(),
    }
}

fn sessions(projects: &Path) -> Vec<Session> {
    let now = SystemTime::now();
    let mut out = Vec::new();
    let Ok(dirs) = std::fs::read_dir(projects) else { return out };
    for dir in dirs.flatten() {
        let Ok(files) = std::fs::read_dir(dir.path()) else { continue };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let Some(idle) = f
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .map(|d| d.as_secs())
            else {
                continue;
            };
            if idle <= ACTIVE_SECS {
                let project = project_of(&path).unwrap_or_else(|| dir.file_name().to_string_lossy().to_string());
                out.push(Session { project, idle_s: idle });
            }
        }
    }
    out.sort_by_key(|s| s.idle_s);
    out
}

/// Nom du dossier de travail, lu dans le champ "cwd" de la fin du transcript.
fn project_of(path: &Path) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(16_384))).ok()?;
    let mut tail = Vec::new();
    f.read_to_end(&mut tail).ok()?;
    let text = String::from_utf8_lossy(&tail);
    let start = text.rfind("\"cwd\":\"")? + 7;
    let end = start + text[start..].find('"')?;
    let cwd = text[start..end].replace("\\\\", "\\");
    cwd.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().map(str::to_string).filter(|s| !s.is_empty())
}
