//! Lecture en cours via les contrôles média de Windows (Spotify, navigateurs, Deezer...).
//!
//! Tout part des événements du gestionnaire de sessions (GSMTC) : ses gestionnaires d'événements
//! ne font que réveiller le thread de travail, qui relit ce qui a changé et le publie aussitôt.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use tauri::AppHandle;
use windows::core::{IUnknown, Interface, RuntimeType};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapDecoder, BitmapEncoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform,
    ColorManagementMode, ExifOrientationMode,
};
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Props,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status, MediaPropertiesChangedEventArgs,
    PlaybackInfoChangedEventArgs, SessionsChangedEventArgs, TimelinePropertiesChangedEventArgs,
};
use windows::Storage::Streams::{
    Buffer, DataReader, DataWriter, IInputStream, InMemoryRandomAccessStream, InputStreamOptions,
};
use windows::Win32::Foundation::ERROR_TIMEOUT;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

use crate::{appbar, publish};

/// Écart entre l'époque Windows (1601) et Unix, en unités de 100 ns.
const EPOCH_DIFF: i64 = 116_444_736_000_000_000;
/// Un changement de morceau arrive en rafale (titre, pochette, lecture, position) : on la laisse passer.
const COALESCE: Duration = Duration::from_millis(40);
/// Relecture de secours pendant la lecture, pour les lecteurs qui ne signalent pas tout.
const SAFETY_REFRESH: Duration = Duration::from_secs(5);
/// En pause ou sans lecteur ouvert, les événements suffisent presque toujours.
const IDLE_REFRESH: Duration = Duration::from_secs(30);
/// Attente maximale d'un lecteur : figé (onglet planté, appli suspendue), il ne répondrait jamais.
const CALL_LIMIT: Duration = Duration::from_secs(2);
/// Appels abandonnés encore bloqués au-delà desquels on n'en lance plus.
const MAX_STUCK: usize = 2;
const FULLSCREEN_CHECK: Duration = Duration::from_secs(2);
/// La pochette suit parfois le titre de peu, ou reste un instant celle du morceau précédent.
const ART_RECHECKS: [Duration; 2] = [Duration::from_millis(350), Duration::from_millis(1500)];
/// Filet de sécurité après une commande, pour un lecteur qui ne signalerait pas son nouvel état.
const COMMAND_RECHECK: Duration = Duration::from_millis(500);
const MAX_RECHECKS: usize = 8;
const MAX_ART_BYTES: u32 = 8_000_000;
/// Côté maximal de la pochette publiée : l'îlot l'affiche en 23 px, le panneau en 62 (x2 en haute densité).
const ART_PX: u32 = 160;

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
    /// Pochette de ce morceau (identifiant publié sur "art"), absente tant qu'elle n'a pas pu être lue.
    art: Option<u32>,
}

/// Pochette publiée sur le sujet "art".
#[derive(Serialize)]
struct Cover<'a> {
    id: u32,
    src: &'a str,
}

#[derive(Clone, Copy)]
enum Msg {
    /// Session courante ou liste des sessions modifiée.
    Sessions,
    /// Titre, artiste, album ou pochette.
    Track,
    /// Lecture, commandes disponibles ou position.
    Playback,
    Toggle,
    Next,
    Prev,
}

static TX: OnceLock<Sender<Msg>> = OnceLock::new();

pub fn control(action: &str) {
    let msg = match action {
        "toggle" => Msg::Toggle,
        "next" => Msg::Next,
        "prev" => Msg::Prev,
        _ => return,
    };
    if let Some(tx) = TX.get() {
        let _ = tx.send(msg);
    }
}

pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel();
    let _ = TX.set(tx.clone());
    std::thread::spawn(move || {
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        Worker::new(app, tx).run(rx);
        if com {
            unsafe { CoUninitialize() };
        }
    });
}

/// Ce qu'il reste à relire au prochain passage.
#[derive(Clone, Copy, Default)]
struct Due {
    session: bool,
    track: bool,
    /// Relire la pochette même si on en a déjà une pour ce morceau.
    art: bool,
    playback: bool,
}

impl Due {
    const ALL: Due = Due { session: true, track: true, art: true, playback: true };
    const SESSION: Due = Due { session: true, track: true, art: false, playback: true };
    const TRACK: Due = Due { session: false, track: true, art: true, playback: false };
    const PLAYBACK: Due = Due { session: false, track: false, art: false, playback: true };
    const LATE_ART: Due = Due { session: false, track: true, art: true, playback: true };
    const AFTER_COMMAND: Due = Due { session: false, track: true, art: false, playback: true };

    fn any(self) -> bool {
        self.session || self.track || self.art || self.playback
    }

    fn add(&mut self, other: Due) {
        self.session |= other.session;
        self.track |= other.track;
        self.art |= other.art;
        self.playback |= other.playback;
    }
}

#[derive(Default)]
struct ArtState {
    /// Morceau (titre, artiste, album) auquel `current` se rapporte.
    track: String,
    /// Pochette publiée qui va avec ce morceau.
    current: Option<u32>,
    /// Dernière pochette publiée : son numéro et son empreinte (taille, hash).
    last: u32,
    print: Option<(usize, u64)>,
}

struct Worker {
    app: AppHandle,
    /// Cloné dans chaque gestionnaire d'événement.
    tx: Sender<Msg>,
    manager: Option<ManagerWatch>,
    current: Option<SessionWatch>,
    /// Dernier état lu de la session courante.
    media: Option<Media>,
    art: ArtState,
    /// Relectures programmées : quand, et quoi relire.
    rechecks: Vec<(Instant, Due)>,
}

impl Worker {
    fn new(app: AppHandle, tx: Sender<Msg>) -> Self {
        Worker { app, tx, manager: None, current: None, media: None, art: ArtState::default(), rechecks: Vec::new() }
    }

    fn run(mut self, rx: Receiver<Msg>) {
        let mut due = Due::ALL;
        let mut next_refresh = Instant::now();
        loop {
            let fullscreen = appbar::fullscreen_active();
            let wait = if fullscreen {
                FULLSCREEN_CHECK
            } else if due.any() {
                Duration::ZERO
            } else {
                let next = self.rechecks.iter().map(|&(at, _)| at).fold(next_refresh, Instant::min);
                next.saturating_duration_since(Instant::now())
            };
            match rx.recv_timeout(wait) {
                Ok(msg) => {
                    self.receive(msg, &mut due);
                    if !fullscreen && !self.coalesce(&rx, &mut due) {
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            let now = Instant::now();
            if now >= next_refresh {
                due.add(Due::SESSION);
            }
            self.rechecks.retain(|&(at, what)| {
                if at > now {
                    return true;
                }
                due.add(what);
                false
            });
            // En jeu, rien n'est lu : l'état reste à relire jusqu'à la sortie du plein écran.
            if !due.any() || appbar::fullscreen_active() {
                continue;
            }
            self.refresh(std::mem::take(&mut due));
            let playing = self.media.as_ref().is_some_and(|m| m.playing);
            let idle = self.manager.is_some() && !playing;
            next_refresh = Instant::now() + if idle { IDLE_REFRESH } else { SAFETY_REFRESH };
        }
    }

    fn receive(&mut self, msg: Msg, due: &mut Due) {
        match msg {
            Msg::Sessions => due.add(Due::SESSION),
            Msg::Track => due.add(Due::TRACK),
            Msg::Playback => due.add(Due::PLAYBACK),
            Msg::Toggle | Msg::Next | Msg::Prev => self.command(msg),
        }
    }

    /// Laisse arriver le reste de la rafale pour n'en faire qu'une lecture. Faux si le canal est fermé.
    fn coalesce(&mut self, rx: &Receiver<Msg>, due: &mut Due) -> bool {
        let until = Instant::now() + COALESCE;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            match rx.recv_timeout(left) {
                Ok(msg) => self.receive(msg, due),
                Err(RecvTimeoutError::Timeout) => return true,
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    /// Envoie la commande sans attendre la réponse du lecteur : son nouvel état arrive par les événements.
    fn command(&mut self, msg: Msg) {
        let sent = match (&self.current, &self.manager) {
            (Some(watch), _) => send_command(&watch.session, msg),
            (None, Some(watch)) => watch.manager.GetCurrentSession().and_then(|s| send_command(&s, msg)),
            (None, None) => return,
        };
        if sent.is_ok() {
            self.recheck_in(COMMAND_RECHECK, Due::AFTER_COMMAND);
        }
    }

    fn recheck_in(&mut self, delay: Duration, what: Due) {
        // Borné : une rafale de clics ne doit pas empiler les relectures.
        if self.rechecks.len() < MAX_RECHECKS {
            self.rechecks.push((Instant::now() + delay, what));
        }
    }

    fn refresh(&mut self, due: Due) {
        if (due.session || self.manager.is_none()) && self.follow_current() {
            self.media = None;
        }
        let Some(session) = self.current.as_ref().map(|w| w.session.clone()) else {
            self.media = None;
            publish(&self.app, "media", &None::<Media>);
            return;
        };
        let fresh = self.media.is_none();
        let mut media = self.media.take().unwrap_or_else(|| Media { source: source_of(&session), ..Media::default() });
        let mut ok = true;
        if fresh || due.track {
            ok &= self.read_track(&session, &mut media, fresh || due.art).is_ok();
        }
        if fresh || due.playback {
            ok &= read_playback(&session, &mut media).is_ok();
        }
        // Session toute neuve illisible : rien de fiable à montrer, le prochain passage réessaiera.
        if fresh && !ok {
            publish(&self.app, "media", &None::<Media>);
            return;
        }
        media.art = self.art.current;
        publish(&self.app, "media", &Some(&media));
        self.media = Some(media);
    }

    /// Suit la session courante (abonnements compris) ; vrai si elle a changé.
    fn follow_current(&mut self) -> bool {
        if self.manager.is_none() {
            self.manager = ManagerWatch::open(&self.tx);
        }
        let current = self.manager.as_ref().and_then(|w| w.manager.GetCurrentSession().ok());
        let same = match (&self.current, &current) {
            (Some(watch), Some(s)) => same_object(&watch.session, s),
            (None, None) => true,
            _ => false,
        };
        if !same {
            // L'ancienne session se désabonne en étant libérée.
            self.current = current.map(|s| SessionWatch::new(s, &self.tx));
        }
        !same
    }

    /// Titre, artiste et album. La pochette est relue dans le même passage si le morceau change,
    /// si elle manque encore ou si `art` le demande, pour partir avec les métadonnées.
    fn read_track(&mut self, session: &Session, media: &mut Media, art: bool) -> windows::core::Result<()> {
        let s = session.clone();
        let props = bounded(move || s.TryGetMediaPropertiesAsync()?.join())?;
        let title = props.Title()?.to_string();
        let artist = props.Artist()?.to_string();
        let album = props.AlbumTitle()?.to_string();
        let key = format!("{title}\u{1}{artist}\u{1}{album}");
        let changed = key != self.art.track;
        if changed {
            self.art.track = key;
            self.art.current = None;
            for delay in ART_RECHECKS {
                self.recheck_in(delay, Due::LATE_ART);
            }
        }
        if art || changed || self.art.current.is_none() {
            self.read_art(&props);
        }
        media.title = title;
        media.artist = artist;
        media.album = album;
        Ok(())
    }

    /// Publie la pochette si l'image est nouvelle ; la même image (même album) n'est ni réencodée ni renvoyée.
    /// Un échec de lecture garde la pochette déjà associée au morceau.
    fn read_art(&mut self, props: &Props) {
        let props = props.clone();
        let Ok((bytes, mime)) = bounded(move || thumbnail(&props).ok_or_else(windows::core::Error::empty)) else {
            return;
        };
        let print = fingerprint(&bytes);
        if self.art.print != Some(print) {
            self.art.last = self.art.last.wrapping_add(1);
            self.art.print = Some(print);
            let (bytes, mime) = shrink(&bytes, &mime).unwrap_or((bytes, mime));
            let src = format!("data:{mime};base64,{}", STANDARD.encode(&bytes));
            publish(&self.app, "art", &Cover { id: self.art.last, src: &src });
        }
        self.art.current = Some(self.art.last);
    }
}

/// Gestionnaire de sessions et ses abonnements, retirés à la libération.
struct ManagerWatch {
    manager: Manager,
    tokens: [Option<i64>; 2],
}

impl ManagerWatch {
    fn open(tx: &Sender<Msg>) -> Option<Self> {
        let manager = bounded(|| Manager::RequestAsync()?.join()).ok()?;
        let on_current = signal::<Manager, CurrentSessionChangedEventArgs>(tx, Msg::Sessions);
        let on_list = signal::<Manager, SessionsChangedEventArgs>(tx, Msg::Sessions);
        let tokens = [manager.CurrentSessionChanged(&on_current).ok(), manager.SessionsChanged(&on_list).ok()];
        Some(ManagerWatch { manager, tokens })
    }
}

impl Drop for ManagerWatch {
    fn drop(&mut self) {
        let [current, list] = self.tokens;
        if let Some(token) = current {
            let _ = self.manager.RemoveCurrentSessionChanged(token);
        }
        if let Some(token) = list {
            let _ = self.manager.RemoveSessionsChanged(token);
        }
    }
}

/// Session suivie et ses abonnements, retirés quand on cesse de la suivre.
struct SessionWatch {
    session: Session,
    tokens: [Option<i64>; 3],
}

impl SessionWatch {
    fn new(session: Session, tx: &Sender<Msg>) -> Self {
        let on_props = signal::<Session, MediaPropertiesChangedEventArgs>(tx, Msg::Track);
        let on_playback = signal::<Session, PlaybackInfoChangedEventArgs>(tx, Msg::Playback);
        let on_timeline = signal::<Session, TimelinePropertiesChangedEventArgs>(tx, Msg::Playback);
        let tokens = [
            session.MediaPropertiesChanged(&on_props).ok(),
            session.PlaybackInfoChanged(&on_playback).ok(),
            session.TimelinePropertiesChanged(&on_timeline).ok(),
        ];
        SessionWatch { session, tokens }
    }
}

impl Drop for SessionWatch {
    fn drop(&mut self) {
        let [props, playback, timeline] = self.tokens;
        if let Some(token) = props {
            let _ = self.session.RemoveMediaPropertiesChanged(token);
        }
        if let Some(token) = playback {
            let _ = self.session.RemovePlaybackInfoChanged(token);
        }
        if let Some(token) = timeline {
            let _ = self.session.RemoveTimelinePropertiesChanged(token);
        }
    }
}

/// Gestionnaire d'événement WinRT (appelé sur un thread quelconque) qui ne fait que réveiller le thread de travail.
fn signal<S, A>(tx: &Sender<Msg>, msg: Msg) -> TypedEventHandler<S, A>
where
    S: RuntimeType + 'static,
    A: RuntimeType + 'static,
{
    let tx = tx.clone();
    TypedEventHandler::new(move |_, _| {
        let _ = tx.send(msg);
        Ok(())
    })
}

/// Identité COM : deux références désignent le même objet si leurs `IUnknown` coïncident.
fn same_object(a: &Session, b: &Session) -> bool {
    if a.as_raw() == b.as_raw() {
        return true;
    }
    match (a.cast::<IUnknown>(), b.cast::<IUnknown>()) {
        (Ok(a), Ok(b)) => a.as_raw() == b.as_raw(),
        _ => false,
    }
}

fn send_command(s: &Session, msg: Msg) -> windows::core::Result<()> {
    // L'opération suit son cours même si on lâche sa référence sans l'attendre.
    match msg {
        Msg::Toggle => s.TryTogglePlayPauseAsync().map(|_| ()),
        Msg::Next => s.TrySkipNextAsync().map(|_| ()),
        Msg::Prev => s.TrySkipPreviousAsync().map(|_| ()),
        Msg::Sessions | Msg::Track | Msg::Playback => Ok(()),
    }
}

/// Lecture/pause, commandes disponibles et position. Lectures synchrones (état en cache dans la session).
fn read_playback(s: &Session, media: &mut Media) -> windows::core::Result<()> {
    let playback = s.GetPlaybackInfo()?;
    let controls = playback.Controls()?;
    let timeline = s.GetTimelineProperties()?;
    let playing = playback.PlaybackStatus()? == Status::Playing;
    let position_ms = timeline.Position()?.Duration / 10_000;
    let duration_ms = timeline.EndTime()?.Duration / 10_000;
    let updated = timeline.LastUpdatedTime()?.UniversalTime;
    media.playing = playing;
    media.position_ms = position_ms;
    media.duration_ms = duration_ms;
    media.updated_ms = if updated > EPOCH_DIFF { (updated - EPOCH_DIFF) / 10_000 } else { 0 };
    media.can_prev = controls.IsPreviousEnabled().unwrap_or(false);
    media.can_next = controls.IsNextEnabled().unwrap_or(false);
    Ok(())
}

fn source_of(s: &Session) -> String {
    s.SourceAppUserModelId().map(|id| source_name(&id.to_string())).unwrap_or_default()
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

/// Appel au lecteur sur un thread à part, attendu au plus `CALL_LIMIT`. Un lecteur figé ne répond jamais :
/// son appel est abandonné (le thread reste bloqué avec lui) et le thread média continue.
fn bounded<T: Send + 'static>(
    call: impl FnOnce() -> windows::core::Result<T> + Send + 'static,
) -> windows::core::Result<T> {
    static STUCK: AtomicUsize = AtomicUsize::new(0);
    let timeout = || windows::core::Error::from(ERROR_TIMEOUT.to_hresult());
    if STUCK.load(Relaxed) >= MAX_STUCK {
        return Err(timeout());
    }
    let (tx, rx) = mpsc::sync_channel(1);
    STUCK.fetch_add(1, Relaxed);
    let spawned = std::thread::Builder::new().name("media-call".into()).spawn(move || {
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let _ = tx.send(call());
        if com {
            unsafe { CoUninitialize() };
        }
        STUCK.fetch_sub(1, Relaxed);
    });
    if spawned.is_err() {
        STUCK.fetch_sub(1, Relaxed);
        return Err(timeout());
    }
    rx.recv_timeout(CALL_LIMIT).unwrap_or_else(|_| Err(timeout()))
}

/// Octets bruts de la pochette et leur type MIME.
fn thumbnail(p: &Props) -> Option<(Vec<u8>, String)> {
    let stream = p.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let size = u32::try_from(stream.Size().ok()?).ok()?;
    if size == 0 || size > MAX_ART_BYTES {
        return None;
    }
    let bytes = read_all(&stream.GetInputStreamAt(0).ok()?, size)?;
    let mime = stream.ContentType().map(|m| m.to_string()).unwrap_or_default();
    let mime = if mime.starts_with("image/") { mime } else { "image/png".into() };
    Some((bytes, mime))
}

fn read_all(input: &IInputStream, size: u32) -> Option<Vec<u8>> {
    let buffer = Buffer::Create(size).ok()?;
    let filled = input.ReadAsync(&buffer, size, InputStreamOptions::None).ok()?.join().ok()?;
    let reader = DataReader::FromBuffer(&filled).ok()?;
    let mut bytes = vec![0u8; filled.Length().ok()? as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    (!bytes.is_empty()).then_some(bytes)
}

/// Réduit une grande pochette à `ART_PX` de côté : une image de plusieurs Mo, encodée en base64 puis
/// décodée par la WebView, coûterait des dizaines de Mo à chaque morceau. None si elle est déjà petite.
fn shrink(bytes: &[u8], mime: &str) -> Option<(Vec<u8>, String)> {
    let input = InMemoryRandomAccessStream::new().ok()?;
    let writer = DataWriter::CreateDataWriter(&input).ok()?;
    writer.WriteBytes(bytes).ok()?;
    writer.StoreAsync().ok()?.join().ok()?;
    writer.DetachStream().ok()?;
    input.Seek(0).ok()?;
    let decoder = BitmapDecoder::CreateAsync(&input).ok()?.join().ok()?;
    let (w, h) = (decoder.PixelWidth().ok()?, decoder.PixelHeight().ok()?);
    if w.max(h) <= ART_PX || w == 0 || h == 0 {
        return None;
    }
    let (sw, sh) = if w >= h { (ART_PX, (h * ART_PX / w).max(1)) } else { ((w * ART_PX / h).max(1), ART_PX) };
    let transform = BitmapTransform::new().ok()?;
    transform.SetScaledWidth(sw).ok()?;
    transform.SetScaledHeight(sh).ok()?;
    transform.SetInterpolationMode(BitmapInterpolationMode::Fant).ok()?;
    // Le JPEG reste JPEG (plus léger) ; le reste passe en PNG, qui garde la transparence.
    let jpeg = mime == "image/jpeg";
    let alpha = if jpeg { BitmapAlphaMode::Ignore } else { BitmapAlphaMode::Premultiplied };
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            alpha,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )
        .ok()?
        .join()
        .ok()?
        .DetachPixelData()
        .ok()?;
    let output = InMemoryRandomAccessStream::new().ok()?;
    let id = if jpeg { BitmapEncoder::JpegEncoderId() } else { BitmapEncoder::PngEncoderId() }.ok()?;
    let encoder = BitmapEncoder::CreateAsync(id, &output).ok()?.join().ok()?;
    encoder.SetPixelData(BitmapPixelFormat::Bgra8, alpha, sw, sh, 96.0, 96.0, &pixels).ok()?;
    encoder.FlushAsync().ok()?.join().ok()?;
    let size = u32::try_from(output.Size().ok()?).ok()?;
    let small = read_all(&output.GetInputStreamAt(0).ok()?, size)?;
    Some((small, if jpeg { mime.to_string() } else { "image/png".into() }))
}

/// Empreinte d'une image, pour reconnaître la même pochette sans la réencoder.
fn fingerprint(bytes: &[u8]) -> (usize, u64) {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    (bytes.len(), hasher.finish())
}
