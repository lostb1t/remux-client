//! One Cast session, on its own thread: it connects, starts the Default Media Receiver, loads
//! what it is told to, and keeps what the receiver reports where the player can read it.

use std::net::IpAddr;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::link::*;

/// The receiver that plays what it is given, with the picture and the sound and nothing of its own.
const DEFAULT_RECEIVER: &str = "CC1AD845";
const HEARTBEAT_EVERY: Duration = Duration::from_secs(5);
const STATUS_EVERY: Duration = Duration::from_secs(1);
/// Nothing at all from the device for this long: it is gone.
const SILENCE_LIMIT: Duration = Duration::from_secs(20);

pub struct Subtitle {
    /// The track's number, as the player knows it.
    pub index: i64,
    pub label: String,
    pub language: Option<String>,
    pub url: String,
}

pub enum Command {
    Load { url: String, content_type: String, start_ms: f64, subtitles: Vec<Subtitle>, active_subtitle: Option<i64> },
    Pause(bool),
    Seek(f64),
    Volume { level: f64, muted: bool },
    Rate(f64),
    Subtitle(Option<i64>),
    /// Stops what plays, and the receiver with it, and ends the session.
    Stop,
}

/// What the receiver last said.
#[derive(Clone, Debug)]
pub struct Status {
    pub position_ms: f64,
    pub duration_ms: f64,
    pub paused: bool,
    pub ended: bool,
    pub volume: f64,
    pub muted: bool,
    pub rate: f64,
    pub error: Option<String>,
    /// Whether the position moves; it is then worked out from when it was reported.
    pub playing: bool,
    pub reported: Instant,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            position_ms: 0.0,
            duration_ms: 0.0,
            paused: true,
            ended: false,
            volume: 1.0,
            muted: false,
            rate: 1.0,
            error: None,
            playing: false,
            reported: Instant::now(),
        }
    }
}

impl Status {
    pub fn position_now(&self) -> f64 {
        if self.playing {
            self.position_ms + self.reported.elapsed().as_secs_f64() * 1000.0 * self.rate
        } else {
            self.position_ms
        }
    }
}

struct Loading {
    url: String,
    content_type: String,
    start_ms: f64,
    subtitles: Vec<Subtitle>,
    active_subtitle: Option<i64>,
}

pub fn run(host: IpAddr, port: u16, commands: Receiver<Command>, shared: Arc<Mutex<Status>>) {
    if let Err(err) = session(host, port, &commands, &shared) {
        log::error!("cast session ended: {err}");
        let mut status = shared.lock().unwrap();
        status.error.get_or_insert_with(|| format!("Lost the connection to the Chromecast ({err})"));
        status.playing = false;
    }
}

fn session(
    host: IpAddr,
    port: u16,
    commands: &Receiver<Command>,
    shared: &Arc<Mutex<Status>>,
) -> std::io::Result<()> {
    let mut link = Link::connect(host, port)?;
    let mut request = 0u32;
    let mut next_request = move || {
        request += 1;
        request
    };

    link.send(NS_CONNECTION, RECEIVER, &json!({"type": "CONNECT"}))?;
    link.send(NS_RECEIVER, RECEIVER, &json!({"type": "LAUNCH", "appId": DEFAULT_RECEIVER, "requestId": next_request()}))?;

    // The application the receiver runs for us, once it is up.
    let mut app: Option<(String, String)> = None;
    let mut media_session: Option<i64> = None;
    let mut waiting: Option<Loading> = None;
    let mut subtitle_ids: Vec<i64> = Vec::new();
    let mut last_heard = Instant::now();
    let mut last_heartbeat = Instant::now();
    let mut last_status = Instant::now();

    loop {
        // What the player asks for.
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Stop => {
                    if let Some((_, session_id)) = &app {
                        let _ = link.send(
                            NS_RECEIVER,
                            RECEIVER,
                            &json!({"type": "STOP", "sessionId": session_id, "requestId": next_request()}),
                        );
                    }
                    return Ok(());
                }
                Command::Load { url, content_type, start_ms, subtitles, active_subtitle } => {
                    let loading = Loading { url, content_type, start_ms, subtitles, active_subtitle };
                    media_session = None;
                    {
                        let mut status = shared.lock().unwrap();
                        let (volume, muted) = (status.volume, status.muted);
                        *status = Status { volume, muted, position_ms: loading.start_ms, ..Status::default() };
                    }
                    match &app {
                        Some((transport, session_id)) => {
                            subtitle_ids = loading.subtitles.iter().map(|s| s.index).collect();
                            send_load(&mut link, transport, session_id, &loading, next_request())?;
                        }
                        None => waiting = Some(loading),
                    }
                }
                Command::Volume { level, muted } => {
                    link.send(
                        NS_RECEIVER,
                        RECEIVER,
                        &json!({"type": "SET_VOLUME", "volume": {"level": level, "muted": muted}, "requestId": next_request()}),
                    )?;
                    let mut status = shared.lock().unwrap();
                    status.volume = level;
                    status.muted = muted;
                }
                other => {
                    let (Some((transport, _)), Some(id)) = (&app, media_session) else {
                        continue;
                    };
                    let payload = match other {
                        Command::Pause(true) => json!({"type": "PAUSE"}),
                        Command::Pause(false) => json!({"type": "PLAY"}),
                        Command::Seek(ms) => {
                            let mut status = shared.lock().unwrap();
                            status.position_ms = ms;
                            status.reported = Instant::now();
                            json!({"type": "SEEK", "currentTime": ms / 1000.0})
                        }
                        Command::Rate(rate) => json!({"type": "SET_PLAYBACK_RATE", "playbackRate": rate}),
                        Command::Subtitle(index) => {
                            let active: Vec<i64> = index.filter(|i| subtitle_ids.contains(i)).into_iter().collect();
                            json!({"type": "EDIT_TRACKS_INFO", "activeTrackIds": active})
                        }
                        _ => continue,
                    };
                    let mut payload = payload;
                    payload["mediaSessionId"] = json!(id);
                    payload["requestId"] = json!(next_request());
                    link.send(NS_MEDIA, transport, &payload)?;
                }
            }
        }

        // What the receiver says.
        if let Some((namespace, _source, payload)) = link.receive()? {
            last_heard = Instant::now();
            match (namespace.as_str(), payload["type"].as_str().unwrap_or_default()) {
                (NS_HEARTBEAT, "PING") => link.send(NS_HEARTBEAT, RECEIVER, &json!({"type": "PONG"}))?,
                (NS_CONNECTION, "CLOSE") => {
                    return Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, "the device closed the session"));
                }
                (NS_RECEIVER, "RECEIVER_STATUS") => {
                    let status = &payload["status"];
                    if let Some(volume) = status["volume"]["level"].as_f64() {
                        let mut shared = shared.lock().unwrap();
                        shared.volume = volume;
                        shared.muted = status["volume"]["muted"].as_bool().unwrap_or(false);
                    }
                    let running = status["applications"]
                        .as_array()
                        .and_then(|apps| apps.iter().find(|a| a["appId"] == DEFAULT_RECEIVER));
                    if let (Some(running), None) = (running, &app) {
                        let transport = running["transportId"].as_str().unwrap_or_default().to_string();
                        let session_id = running["sessionId"].as_str().unwrap_or_default().to_string();
                        link.send(NS_CONNECTION, &transport, &json!({"type": "CONNECT"}))?;
                        if let Some(loading) = waiting.take() {
                            subtitle_ids = loading.subtitles.iter().map(|s| s.index).collect();
                            send_load(&mut link, &transport, &session_id, &loading, next_request())?;
                        }
                        app = Some((transport, session_id));
                    } else if running.is_none() && app.is_some() {
                        // Something else took the screen, or the viewer closed it on the TV.
                        return Err(std::io::Error::other("the Chromecast stopped the player"));
                    }
                }
                (NS_MEDIA, "MEDIA_STATUS") => {
                    if let Some(entry) = payload["status"].as_array().and_then(|s| s.first()) {
                        media_session = entry["mediaSessionId"].as_i64().or(media_session);
                        let state = entry["playerState"].as_str().unwrap_or_default();
                        let reason = entry["idleReason"].as_str();
                        let mut status = shared.lock().unwrap();
                        if let Some(time) = entry["currentTime"].as_f64() {
                            status.position_ms = time * 1000.0;
                        }
                        if let Some(duration) = entry["media"]["duration"].as_f64() {
                            status.duration_ms = duration * 1000.0;
                        }
                        status.rate = entry["playbackRate"].as_f64().unwrap_or(1.0);
                        status.playing = state == "PLAYING";
                        status.paused = state == "PAUSED" || state == "IDLE";
                        status.ended = state == "IDLE" && reason == Some("FINISHED");
                        if state == "IDLE" && matches!(reason, Some("ERROR") | Some("INTERRUPTED")) {
                            status.error = Some("The Chromecast could not play this".to_string());
                        }
                        status.reported = Instant::now();
                    }
                }
                (NS_MEDIA, "LOAD_FAILED") | (NS_MEDIA, "INVALID_REQUEST") | (NS_MEDIA, "LOAD_CANCELLED") => {
                    log::warn!("cast: {payload}");
                    shared.lock().unwrap().error = Some("The Chromecast could not load this".to_string());
                }
                _ => {}
            }
        }

        if last_heartbeat.elapsed() >= HEARTBEAT_EVERY {
            link.send(NS_HEARTBEAT, RECEIVER, &json!({"type": "PING"}))?;
            last_heartbeat = Instant::now();
        }
        if last_status.elapsed() >= STATUS_EVERY {
            if let (Some((transport, _)), Some(id)) = (&app, media_session) {
                link.send(NS_MEDIA, transport, &json!({"type": "GET_STATUS", "mediaSessionId": id, "requestId": next_request()}))?;
            }
            last_status = Instant::now();
        }
        if last_heard.elapsed() > SILENCE_LIMIT {
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "no answer from the device"));
        }
    }
}

fn send_load(link: &mut Link, transport: &str, session_id: &str, load: &Loading, request: u32) -> std::io::Result<()> {
    let tracks: Vec<Value> = load
        .subtitles
        .iter()
        .map(|s| {
            json!({
                "trackId": s.index,
                "type": "TEXT",
                "trackContentId": s.url,
                "trackContentType": "text/vtt",
                "subtype": "SUBTITLES",
                "name": s.label,
                "language": s.language.clone().unwrap_or_default(),
            })
        })
        .collect();
    let active: Vec<i64> = load.active_subtitle.into_iter().collect();
    let mut media = json!({
        "contentId": load.url,
        "streamType": "BUFFERED",
        "contentType": load.content_type,
    });
    if !tracks.is_empty() {
        media["tracks"] = json!(tracks);
    }
    link.send(
        NS_MEDIA,
        transport,
        &json!({
            "type": "LOAD",
            "requestId": request,
            "sessionId": session_id,
            "media": media,
            "autoplay": true,
            "currentTime": load.start_ms / 1000.0,
            "activeTrackIds": active,
        }),
    )
}

/// Starts the session's thread.
pub fn spawn(host: IpAddr, port: u16, commands: Receiver<Command>, shared: Arc<Mutex<Status>>) {
    std::thread::Builder::new()
        .name("cast".into())
        .spawn(move || run(host, port, commands, shared))
        .ok();
}
