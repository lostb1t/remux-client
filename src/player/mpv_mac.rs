//! The video player on a Mac: mpv (the `RemuxMpv` framework, `macos/RemuxMpv`, built by
//! `scripts/build-macos-framework.sh`) as the native component `remux.mpv`, drawing on a Metal layer
//! in a view the framework makes. It takes what `remux.exo` takes on Android (`player/exo.rs`): the
//! props `source`, `paused`, `subtitle`, `audio` and `rate`, the command `seek`, and it sends the
//! event `state`, so the player page (`pages/player.rs`) is the same on both.
//!
//! mpv plays the file as it is: every codec, its audio tracks and its subtitles (also the pictures
//! of PGS, which it draws itself), so the server need not transcode. It has no events here: its
//! state is read four times a second (`remux_mpv_snapshot`).

use std::ffi::{CString, c_char};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use nsh_core::JsonValue;
use nsh_platform_macos::objc2::rc::Retained;
use nsh_platform_macos::objc2_app_kit::NSView;
use nsh_platform_macos::{ComponentContext, Components, NativeComponent};
use serde_json::json;

/// The name the player page draws the video with.
pub const NAME: &str = "remux.mpv";

/// The framework's C functions (`macos/RemuxMpv/Sources/RemuxMpv/Bridge.swift`).
struct Bridge {
    _library: libloading::Library,
    view: unsafe extern "C" fn() -> *mut std::ffi::c_void,
    load: unsafe extern "C" fn(*const c_char, f64, f64, bool, f64, *const c_char),
    close: unsafe extern "C" fn(),
    pause: unsafe extern "C" fn(bool),
    seek: unsafe extern "C" fn(f64),
    rate: unsafe extern "C" fn(f64),
    volume: unsafe extern "C" fn(f64, bool),
    fullscreen: unsafe extern "C" fn(),
    subtitle: unsafe extern "C" fn(i64),
    audio: unsafe extern "C" fn(i64),
    keep_awake: unsafe extern "C" fn(bool),
    snapshot: unsafe extern "C" fn(*mut c_char, i32) -> i32,
}

// SAFETY: the functions are plain C functions of a library that stays loaded; the framework
// does its AppKit work on the main thread itself.
unsafe impl Send for Bridge {}
unsafe impl Sync for Bridge {}

/// Where the framework is: in the app's Frameworks folder, where `REMUX_MPV_FRAMEWORK_DIR` says,
/// or (a run without an app) where it was built.
fn candidates() -> Vec<std::path::PathBuf> {
    let mut list = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        list.push(dir.join("../Frameworks"));
    }
    if let Ok(dir) = std::env::var("REMUX_MPV_FRAMEWORK_DIR") {
        list.push(dir.into());
    }
    list.push(env!("REMUX_MPV_BUILD_DIR").into());
    list.into_iter().map(|dir| dir.join("RemuxMpv.framework/RemuxMpv")).collect()
}

/// The framework, loaded once; `None` where it is not (a build without it), and the player says so.
fn bridge() -> Option<&'static Bridge> {
    static BRIDGE: std::sync::OnceLock<Option<Bridge>> = std::sync::OnceLock::new();
    BRIDGE
        .get_or_init(|| {
            for path in candidates() {
                if !path.exists() {
                    continue;
                }
                // SAFETY: loading the framework runs its initialisers, which only set up Swift.
                let library = match unsafe { libloading::Library::new(&path) } {
                    Ok(library) => library,
                    Err(error) => {
                        log::error!("mpv framework at {} did not load: {error}", path.display());
                        continue;
                    }
                };
                macro_rules! function {
                    ($name:literal) => {
                        // SAFETY: the symbol has the signature of the field it goes in (Bridge.swift).
                        match unsafe { library.get($name) } {
                            Ok(symbol) => *symbol,
                            Err(error) => {
                                log::error!("mpv framework at {} lacks a function: {error}", path.display());
                                continue;
                            }
                        }
                    };
                }
                let bridge = Bridge {
                    view: function!(b"remux_mpv_view\0"),
                    load: function!(b"remux_mpv_load\0"),
                    close: function!(b"remux_mpv_close\0"),
                    pause: function!(b"remux_mpv_pause\0"),
                    seek: function!(b"remux_mpv_seek\0"),
                    rate: function!(b"remux_mpv_rate\0"),
                    volume: function!(b"remux_mpv_volume\0"),
                    fullscreen: function!(b"remux_mpv_fullscreen\0"),
                    subtitle: function!(b"remux_mpv_subtitle\0"),
                    audio: function!(b"remux_mpv_audio\0"),
                    keep_awake: function!(b"remux_mpv_keep_awake\0"),
                    snapshot: function!(b"remux_mpv_snapshot\0"),
                    _library: library,
                };
                log::info!("mpv framework: {}", path.display());
                return Some(bridge);
            }
            log::error!("no mpv framework: scripts/build-macos-framework.sh builds it");
            None
        })
        .as_ref()
}

pub fn register(components: &Components) {
    components.register(NAME, Rc::new(|context: ComponentContext| -> Box<dyn NativeComponent> { Box::new(Mpv::new(context)) }));
}

struct Mpv {
    bridge: Option<&'static Bridge>,
    view: Retained<NSView>,
    /// While the component lives: the thread that reads mpv's state stops with it.
    alive: Arc<AtomicBool>,
    rate: f64,
    volume: f64,
    muted: bool,
}

/// The app's window into full screen, or out of it (the player's button and `f`).
pub fn toggle_fullscreen() {
    if let Some(bridge) = bridge() {
        unsafe { (bridge.fullscreen)() };
    }
}

/// A C string; a stray NUL in a title is dropped rather than the call.
fn c_string(text: &str) -> CString {
    CString::new(text.replace('\0', "")).unwrap_or_default()
}

impl Mpv {
    fn new(context: ComponentContext) -> Self {
        let alive = Arc::new(AtomicBool::new(true));
        let Some(bridge) = bridge() else {
            // No player in this build: an empty view, and the player page says why.
            context.emit("state", json!({ "error": "This build has no video player (the mpv framework is missing)" }));
            let mtm = nsh_platform_macos::objc2::MainThreadMarker::new().expect("components are made on the main thread");
            return Self { bridge: None, view: NSView::new(mtm), alive, rate: 1.0, volume: 1.0, muted: false };
        };
        // SAFETY: `remux_mpv_view` returns the framework's view, retained once more for us.
        let view = unsafe { Retained::from_raw((bridge.view)().cast::<NSView>()) }.expect("the mpv framework gives its view");
        // mpv's state, as ExoView's snapshot: four times a second, sent when it changed.
        let running = alive.clone();
        std::thread::spawn(move || {
            let mut last = String::new();
            while running.load(Ordering::Relaxed) {
                let mut buffer = vec![0 as c_char; 1024];
                // SAFETY: the buffer is as long as said.
                let length = unsafe { (bridge.snapshot)(buffer.as_mut_ptr(), buffer.len() as i32) }.max(0) as usize;
                let text: String = buffer[..length].iter().map(|c| *c as u8 as char).collect();
                if text != last {
                    context.emit("state", state(&text));
                    last = text;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        });
        unsafe { (bridge.keep_awake)(true) };
        Self { bridge: Some(bridge), view, alive, rate: 1.0, volume: 1.0, muted: false }
    }
}

/// `position,duration,paused,ended,volume,muted,rate,buffered end,error` (the framework's) as the
/// state ExoView sends: `position`, `duration`, `ready`, `buffering`, `ended`, `error` and `cues`
/// (none: mpv draws the subtitles).
fn state(text: &str) -> JsonValue {
    let parts: Vec<&str> = text.splitn(9, ',').collect();
    let number = |i: usize| parts.get(i).and_then(|p| p.parse::<f64>().ok()).unwrap_or(0.0);
    let error = parts.get(8).map(|e| e.trim()).unwrap_or_default();
    let (position, duration, ended) = (number(0), number(1), number(3) > 0.5);
    // Ready once the file is open (it has a length, or plays on: a live stream).
    let ready = error.is_empty() && (duration > 0.0 || position > 0.0);
    json!({
        "position": position,
        "duration": duration,
        "ready": ready,
        "buffering": error.is_empty() && !ended && !ready,
        "ended": ended,
        "error": error,
        "cues": "",
    })
}

impl NativeComponent for Mpv {
    fn view(&self) -> Retained<NSView> {
        self.view.clone()
    }

    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        let Some(bridge) = self.bridge else { return };
        match name {
            // What to play, as `ExoView.load` reads it: the stream, where to start, and the
            // WebVTT subtitles the server makes (the file's own mpv reads itself).
            "source" => {
                let Some(source) = value else { return };
                let url = source.get("url").and_then(JsonValue::as_str).unwrap_or_default();
                let start = source.get("startMs").and_then(JsonValue::as_f64).unwrap_or(0.0);
                let subtitles = source
                    .get("subtitles")
                    .and_then(JsonValue::as_array)
                    .map(|list| {
                        list.iter()
                            .map(|s| {
                                let field = |key: &str| s.get(key).and_then(JsonValue::as_str).unwrap_or_default().replace(['\t', '\n'], " ");
                                format!("{}\t{}\t{}\t{}", s.get("index").and_then(JsonValue::as_i64).unwrap_or(-1), field("label"), field("language"), field("url"))
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                let (url, subtitles) = (c_string(url), c_string(&subtitles));
                unsafe { (bridge.load)(url.as_ptr(), start, self.volume, self.muted, self.rate, subtitles.as_ptr()) };
            }
            "paused" => unsafe { (bridge.pause)(value.and_then(JsonValue::as_bool).unwrap_or(false)) },
            "subtitle" => unsafe { (bridge.subtitle)(value.and_then(JsonValue::as_i64).unwrap_or(-1)) },
            "audio" => unsafe { (bridge.audio)(value.and_then(JsonValue::as_i64).unwrap_or(-1)) },
            "volume" | "muted" => {
                match name {
                    "volume" => self.volume = value.and_then(JsonValue::as_f64).unwrap_or(1.0).clamp(0.0, 1.0),
                    _ => self.muted = value.and_then(JsonValue::as_bool).unwrap_or(false),
                }
                unsafe { (bridge.volume)(self.volume, self.muted) };
            }
            "rate" => {
                self.rate = value.and_then(JsonValue::as_f64).unwrap_or(1.0).clamp(0.25, 4.0);
                unsafe { (bridge.rate)(self.rate) };
            }
            _ => {}
        }
    }

    fn command(&mut self, name: &str, args: &JsonValue) {
        let Some(bridge) = self.bridge else { return };
        if name == "seek" {
            unsafe { (bridge.seek)(args.get("ms").and_then(JsonValue::as_f64).unwrap_or(0.0)) };
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        if let Some(bridge) = self.bridge {
            unsafe {
                (bridge.close)();
                (bridge.keep_awake)(false);
            }
        }
    }
}
