//! Casting (Remux's `cast`): playing on a Chromecast, with this device as the remote. The
//! Chromecast fetches the stream from the server itself; what is here finds devices, talks to one
//! over the Cast protocol, and is what the player drives while a device is chosen. The protocol
//! files (`discover`, `link`, `proto`, `session`) are Remux's; on Android the system lets the app
//! hear the network's multicast only with a lock (`DeviceInfo.multicastLock`).

// The web has no network to look on, nor sockets to talk to a device with (Remux's web build
// had no casting either): there the hub stays empty, and nothing is cast.
#[cfg(not(target_arch = "wasm32"))]
mod discover;
#[cfg(not(target_arch = "wasm32"))]
mod link;
#[cfg(not(target_arch = "wasm32"))]
mod proto;
#[cfg(not(target_arch = "wasm32"))]
mod session;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{self, Sender};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;

use nsh_core::prelude::*;

use crate::server::JellyfinServer;
use crate::session::store;

#[cfg(not(target_arch = "wasm32"))]
pub use session::Subtitle;
#[cfg(not(target_arch = "wasm32"))]
use session::{Command, Status};

/// A subtitle the device is given (the web casts nothing; the player still names them).
#[cfg(target_arch = "wasm32")]
pub struct Subtitle {
    pub index: i64,
    pub label: String,
    pub language: Option<String>,
    pub url: String,
}

/// Whether devices are looked for now and then (every 10 seconds): not while something plays,
/// as that is no time for the network to be asked things.
static SEARCHING: AtomicBool = AtomicBool::new(true);

/// How often the network is asked for devices again, so that one switched on later shows up.
const SEARCH_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

fn searching() -> bool {
    SEARCHING.load(Ordering::Relaxed)
}

/// Whether devices are looked for again now and then.
pub fn set_searching(on: bool) {
    SEARCHING.store(on, Ordering::Relaxed);
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// What the device calls itself ("Chromecast Ultra"), which says what it plays.
    pub model: String,
    pub host: IpAddr,
    pub port: u16,
    /// The name of its mDNS record, to take it off the list when it goes.
    pub service: String,
}

/// The session on the server that goes with the device cast to.
#[derive(Clone, Default)]
pub enum CastServer {
    /// Nothing is cast to.
    #[default]
    None,
    /// Being made.
    Pending,
    /// The device has a session of its own, which is what it plays and asks the server with.
    Own(Arc<JellyfinServer>),
    /// The server would not make one: the viewer's own session is used.
    Shared,
}

/// The devices on the network, and the one cast to (with its session on the server).
#[derive(Clone, Default)]
pub struct Hub {
    pub devices: Vec<Device>,
    pub active: Option<Device>,
    pub server: CastServer,
}

pub fn hub() -> Signal<Hub> {
    global::<Hub>()
}

/// The device cast to (tracked where it is read).
pub fn active() -> Option<Device> {
    hub().with(|hub| hub.active.clone())
}

const REMEMBERED: &str = "cast-device";
const TOKENS: &str = "cast-sessions";

/// Starts looking for devices, for as long as the app runs; casts to the one cast to last time
/// once it is found. Once, from the shell.
pub fn start() {
    #[cfg(not(target_arch = "wasm32"))]
    start_discovery();
}

#[cfg(not(target_arch = "wasm32"))]
fn start_discovery() {
    let found = discover::start();
    set_interval(std::time::Duration::from_secs(2), move || {
        let list = found.lock().map(|list| list.clone()).unwrap_or_default();
        if hub().with_untracked(|hub| hub.devices != list) {
            hub().update(|hub| hub.devices = list.clone());
            // The device of last time, back on the network.
            if hub().with_untracked(|hub| hub.active.is_none()) {
                if let Some(device) = store::get(REMEMBERED).and_then(|id| list.into_iter().find(|device| device.id == id)) {
                    pick(device);
                }
            }
        }
    });
}

/// Casts to `device`. The server gets to know it as a device of its own first, so that what the
/// Chromecast plays with doesn't change what the server holds for this one.
pub fn pick(device: Device) {
    let Some(main) = crate::hooks::session() else { return };
    store::set(REMEMBERED, &device.id);
    hub().update(|hub| {
        hub.active = Some(device.clone());
        hub.server = CastServer::Pending;
    });
    spawn(async move {
        let key = format!("{}|{}", main.address(), device.id);
        let mut tokens: HashMap<String, String> = store::get(TOKENS).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        let device_id = format!("{}-{}", crate::app_host::app_host().device_id, device.id);
        let name = format!("{} (Chromecast)", device.name);
        let session = match main.session_for_device(&device_id, &name, tokens.get(&key).cloned()).await {
            Ok(own) => {
                tokens.insert(key, own.token().to_string());
                store::set(TOKENS, &serde_json::to_string(&tokens).unwrap_or_default());
                CastServer::Own(Arc::new(own))
            }
            Err(error) => {
                log::warn!("cast: no session of its own for {name}: {error}");
                CastServer::Shared
            }
        };
        // The viewer may have chosen something else while it was made.
        if hub().with_untracked(|hub| hub.active.as_ref().map(|active| &active.id) == Some(&device.id)) {
            hub().update(|hub| hub.server = session);
        }
    });
}

/// Plays here again.
pub fn stop() {
    store::remove(REMEMBERED);
    hub().update(|hub| {
        hub.active = None;
        hub.server = CastServer::None;
    });
}

/// The session to ask the server things for playing with: the device cast to has its own.
/// `None` while that is being made. Tracked where it is read.
pub fn playback_server() -> Option<Arc<JellyfinServer>> {
    let main = crate::hooks::session()?;
    hub().with(|hub| match &hub.server {
        CastServer::Pending => None,
        CastServer::Own(own) => Some(own.clone()),
        CastServer::None | CastServer::Shared => Some(main),
    })
}

/// What the player drives while a device is chosen: the session with the device, on a thread of
/// its own, and what it last reported.
pub struct CastEngine {
    #[cfg(not(target_arch = "wasm32"))]
    commands: Sender<Command>,
    #[cfg(not(target_arch = "wasm32"))]
    status: Arc<Mutex<Status>>,
}

/// What the device plays, as the player reads it.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub position_ms: f64,
    pub duration_ms: f64,
    pub paused: bool,
    pub playing: bool,
    pub ended: bool,
    pub error: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl CastEngine {
    /// Connects to the device. A connection that fails shows as an error in the snapshot.
    pub fn connect(device: &Device) -> Self {
        let (commands, receiver) = mpsc::channel();
        let status: Arc<Mutex<Status>> = Arc::default();
        session::spawn(device.host, device.port, receiver, status.clone());
        Self { commands, status }
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn load(&self, url: String, hls: bool, start_ms: f64, subtitles: Vec<Subtitle>, active_subtitle: Option<i64>) {
        let content_type = if hls { "application/x-mpegURL" } else { "video/mp4" };
        self.send(Command::Load { url, content_type: content_type.to_string(), start_ms, subtitles, active_subtitle });
    }

    pub fn set_paused(&self, paused: bool) {
        self.send(Command::Pause(paused));
    }

    pub fn seek(&self, ms: f64) {
        self.send(Command::Seek(ms));
    }

    pub fn set_subtitle(&self, index: Option<i64>) {
        self.send(Command::Subtitle(index));
    }

    pub fn snapshot(&self) -> Snapshot {
        let status = self.status.lock().map(|status| status.clone()).unwrap_or_default();
        Snapshot {
            position_ms: status.position_now(),
            duration_ms: status.duration_ms,
            paused: status.paused,
            playing: status.playing,
            ended: status.ended,
            error: status.error,
        }
    }
}

/// The web casts nothing: a device is never chosen there, so this is never made.
#[cfg(target_arch = "wasm32")]
impl CastEngine {
    pub fn connect(_device: &Device) -> Self {
        Self {}
    }
    pub fn load(&self, _url: String, _hls: bool, _start_ms: f64, _subtitles: Vec<Subtitle>, _active_subtitle: Option<i64>) {}
    pub fn set_paused(&self, _paused: bool) {}
    pub fn seek(&self, _ms: f64) {}
    pub fn set_subtitle(&self, _index: Option<i64>) {}
    pub fn snapshot(&self) -> Snapshot {
        Snapshot::default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for CastEngine {
    /// The player closed: what plays on the device stops.
    fn drop(&mut self) {
        self.send(Command::Stop);
    }
}
