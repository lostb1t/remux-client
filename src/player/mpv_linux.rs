//! The video player on Linux: mpv (the system's `libmpv.so.2`, loaded as the player starts) as the
//! native component `remux.mpv`, drawing with its OpenGL render API into a GTK `GLArea`, which NSH
//! lays out with the rest: its controls go over it. It takes what `remux.exo` (Android) and the
//! Mac's `remux.mpv` take: the props `source`, `paused`, `subtitle`, `audio`, `rate`, `volume` and
//! `muted`, the command `seek`, and it sends the event `state`.
//!
//! mpv plays the file as it is: every codec, its audio tracks and its subtitles (PGS too, which it
//! draws), HDR mapped to the screen; the server need not transcode.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nsh_core::JsonValue;
use nsh_platform_linux::gtk4 as gtk;
use nsh_platform_linux::{ComponentContext, Components, NativeComponent};
use gtk::glib;
use gtk::prelude::*;
use serde_json::json;

/// The name the player page draws the video with (the Mac's too).
pub const NAME: &str = "remux.mpv";

// libmpv's C interface (client.h, render.h, render_gl.h): the parts the player uses.
const FORMAT_FLAG: c_int = 3;
const FORMAT_DOUBLE: c_int = 5;
const EVENT_NONE: c_int = 0;
const EVENT_END_FILE: c_int = 7;
const EVENT_FILE_LOADED: c_int = 8;
const END_FILE_REASON_ERROR: c_int = 4;
const RENDER_PARAM_INVALID: c_int = 0;
const RENDER_PARAM_API_TYPE: c_int = 1;
const RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const RENDER_PARAM_OPENGL_FBO: c_int = 3;
const RENDER_PARAM_FLIP_Y: c_int = 4;

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct OpenGlInitParams {
    get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct OpenGlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

#[repr(C)]
struct Event {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct EventEndFile {
    reason: c_int,
    error: c_int,
}

/// libmpv's functions, and EGL's way to OpenGL's (GTK 4 draws with EGL).
struct Api {
    _mpv: libloading::Library,
    _egl: libloading::Library,
    create: unsafe extern "C" fn() -> *mut c_void,
    initialize: unsafe extern "C" fn(*mut c_void) -> c_int,
    terminate_destroy: unsafe extern "C" fn(*mut c_void),
    set_option_string: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int,
    command: unsafe extern "C" fn(*mut c_void, *mut *const c_char) -> c_int,
    set_property_string: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int,
    get_property: unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int,
    get_property_string: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char,
    free: unsafe extern "C" fn(*mut c_void),
    wait_event: unsafe extern "C" fn(*mut c_void, f64) -> *mut Event,
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    render_create: unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *mut RenderParam) -> c_int,
    render_set_update_callback: unsafe extern "C" fn(*mut c_void, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    render_render: unsafe extern "C" fn(*mut c_void, *mut RenderParam) -> c_int,
    render_free: unsafe extern "C" fn(*mut c_void),
    egl_get_proc_address: unsafe extern "C" fn(*const c_char) -> *mut c_void,
}

// SAFETY: plain C functions of libraries that stay loaded.
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

fn api() -> Option<&'static Api> {
    static API: std::sync::OnceLock<Option<Api>> = std::sync::OnceLock::new();
    API.get_or_init(|| {
        // SAFETY: loading the system's libmpv and libEGL runs their initialisers only.
        let mpv = ["libmpv.so.2", "libmpv.so"].iter().find_map(|name| unsafe { libloading::Library::new(name) }.ok());
        let egl = unsafe { libloading::Library::new("libEGL.so.1") }.ok();
        let (Some(mpv), Some(egl)) = (mpv, egl) else {
            log::error!("no libmpv (or libEGL): install mpv's library (libmpv2) for the video player");
            return None;
        };
        macro_rules! function {
            ($library:expr, $name:literal) => {
                // SAFETY: the symbol has the signature of the field it goes in (libmpv's headers).
                match unsafe { $library.get($name) } {
                    Ok(symbol) => *symbol,
                    Err(error) => {
                        log::error!("libmpv lacks a function: {error}");
                        return None;
                    }
                }
            };
        }
        Some(Api {
            create: function!(mpv, b"mpv_create\0"),
            initialize: function!(mpv, b"mpv_initialize\0"),
            terminate_destroy: function!(mpv, b"mpv_terminate_destroy\0"),
            set_option_string: function!(mpv, b"mpv_set_option_string\0"),
            command: function!(mpv, b"mpv_command\0"),
            set_property_string: function!(mpv, b"mpv_set_property_string\0"),
            get_property: function!(mpv, b"mpv_get_property\0"),
            get_property_string: function!(mpv, b"mpv_get_property_string\0"),
            free: function!(mpv, b"mpv_free\0"),
            wait_event: function!(mpv, b"mpv_wait_event\0"),
            error_string: function!(mpv, b"mpv_error_string\0"),
            render_create: function!(mpv, b"mpv_render_context_create\0"),
            render_set_update_callback: function!(mpv, b"mpv_render_context_set_update_callback\0"),
            render_render: function!(mpv, b"mpv_render_context_render\0"),
            render_free: function!(mpv, b"mpv_render_context_free\0"),
            egl_get_proc_address: function!(egl, b"eglGetProcAddress\0"),
            _mpv: mpv,
            _egl: egl,
        })
    })
    .as_ref()
}

fn c(text: &str) -> CString {
    CString::new(text.replace('\0', "")).unwrap_or_default()
}

unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    match api() {
        // SAFETY: EGL's lookup of an OpenGL function by its name.
        Some(api) => unsafe { (api.egl_get_proc_address)(name) },
        None => std::ptr::null_mut(),
    }
}

/// mpv asks for a new frame (from its own thread): the area draws again on its next tick.
unsafe extern "C" fn on_update(flag: *mut c_void) {
    // SAFETY: `flag` is the `AtomicBool` the player keeps alive while mpv has its render context.
    unsafe { &*(flag as *const AtomicBool) }.store(true, Ordering::Relaxed);
}

/// One mpv, and what the page asked of it.
struct Player {
    api: &'static Api,
    mpv: *mut c_void,
    render: Cell<*mut c_void>,
    /// mpv wants to draw again (set from its thread).
    dirty: Arc<AtomicBool>,
    loaded: Cell<bool>,
    file_loaded: Cell<bool>,
    failed: RefCell<Option<String>>,
    wanted_subtitle: Cell<i64>,
    wanted_audio: Cell<i64>,
    pending_subtitles: RefCell<Vec<(i64, String, String, String)>>,
}

impl Player {
    fn new(api: &'static Api) -> Option<Self> {
        // SAFETY: libmpv's calls on the handle it gives.
        unsafe {
            // libmpv refuses to start where numbers are not written as in C ("1.5"), which GTK's
            // locale (the user's language) changes: numbers as in C, as mpv's own player does.
            libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
            let mpv = (api.create)();
            if mpv.is_null() {
                log::error!("mpv_create failed");
                return None;
            }
            for (name, value) in [
                ("vo", "libmpv"),
                ("hwdec", "auto-safe"),
                ("keep-open", "yes"),
                ("osc", "no"),
                ("input-default-bindings", "no"),
                ("sub-auto", "no"),
                ("sub-scale-with-window", "no"),
                ("sub-use-margins", "no"),
                ("tone-mapping", "auto"),
            ] {
                (api.set_option_string)(mpv, c(name).as_ptr(), c(value).as_ptr());
            }
            if (api.initialize)(mpv) < 0 {
                (api.terminate_destroy)(mpv);
                return None;
            }
            Some(Self {
                api,
                mpv,
                render: Cell::new(std::ptr::null_mut()),
                dirty: Arc::new(AtomicBool::new(true)),
                loaded: Cell::new(false),
                file_loaded: Cell::new(false),
                failed: RefCell::new(None),
                wanted_subtitle: Cell::new(-1),
                wanted_audio: Cell::new(-1),
                pending_subtitles: RefCell::new(Vec::new()),
            })
        }
    }

    fn command(&self, args: &[&str]) {
        let owned: Vec<CString> = args.iter().map(|a| c(a)).collect();
        let mut pointers: Vec<*const c_char> = owned.iter().map(|a| a.as_ptr()).collect();
        pointers.push(std::ptr::null());
        // SAFETY: a NULL-terminated array of C strings that live through the call.
        unsafe { (self.api.command)(self.mpv, pointers.as_mut_ptr()) };
    }

    fn set(&self, name: &str, value: &str) {
        // SAFETY: C strings that live through the call.
        unsafe { (self.api.set_property_string)(self.mpv, c(name).as_ptr(), c(value).as_ptr()) };
    }

    fn double(&self, name: &str) -> f64 {
        let mut value = 0f64;
        // SAFETY: a double property into a double.
        let ok = unsafe { (self.api.get_property)(self.mpv, c(name).as_ptr(), FORMAT_DOUBLE, (&mut value as *mut f64).cast()) } >= 0;
        if ok { value } else { 0.0 }
    }

    fn flag(&self, name: &str) -> bool {
        let mut value: c_int = 0;
        // SAFETY: a flag property into an int.
        let ok = unsafe { (self.api.get_property)(self.mpv, c(name).as_ptr(), FORMAT_FLAG, (&mut value as *mut c_int).cast()) } >= 0;
        ok && value != 0
    }

    fn string(&self, name: &str) -> Option<String> {
        // SAFETY: libmpv's string, freed with its own free.
        unsafe {
            let raw = (self.api.get_property_string)(self.mpv, c(name).as_ptr());
            if raw.is_null() {
                return None;
            }
            let text = CStr::from_ptr(raw).to_string_lossy().into_owned();
            (self.api.free)(raw.cast());
            Some(text)
        }
    }

    /// The events since the last look: the file open (its tracks can be picked, the subtitles of
    /// the server added), or failed.
    fn drain(&self) {
        loop {
            // SAFETY: the event libmpv gives, valid until the next call.
            let event = unsafe { &*(self.api.wait_event)(self.mpv, 0.0) };
            match event.event_id {
                EVENT_NONE => return,
                EVENT_FILE_LOADED => {
                    self.file_loaded.set(true);
                    for (index, label, language, url) in self.pending_subtitles.borrow_mut().drain(..) {
                        // "cached": added, not picked; the page picks.
                        self.command(&["sub-add", &url, "cached", &format!("idx:{index} {label}"), &language]);
                    }
                    self.apply_tracks();
                }
                EVENT_END_FILE if !event.data.is_null() => {
                    // SAFETY: an end-file event's data.
                    let end = unsafe { &*(event.data as *const EventEndFile) };
                    if end.reason == END_FILE_REASON_ERROR {
                        // SAFETY: libmpv's static error text.
                        let text = unsafe { CStr::from_ptr((self.api.error_string)(end.error)) }.to_string_lossy().into_owned();
                        *self.failed.borrow_mut() = Some(text);
                    }
                }
                _ => {}
            }
        }
    }

    /// The audio and the subtitle the page asked for, among the tracks mpv lists: the file's by
    /// their index in it (the server's stream index), the added ones by their title.
    fn apply_tracks(&self) {
        if !self.file_loaded.get() {
            return;
        }
        let count = self.double("track-list/count") as i64;
        let audio = self.wanted_audio.get();
        if audio >= 0 {
            for i in 0..count {
                if self.string(&format!("track-list/{i}/type")).as_deref() == Some("audio")
                    && self.double(&format!("track-list/{i}/ff-index")) as i64 == audio
                    && let Some(id) = self.string(&format!("track-list/{i}/id"))
                {
                    self.set("aid", &id);
                    break;
                }
            }
        }
        let subtitle = self.wanted_subtitle.get();
        if subtitle < 0 {
            self.set("sid", "no");
            return;
        }
        for i in 0..count {
            if self.string(&format!("track-list/{i}/type")).as_deref() != Some("sub") {
                continue;
            }
            let matches = if self.flag(&format!("track-list/{i}/external")) {
                self.string(&format!("track-list/{i}/title")).unwrap_or_default().starts_with(&format!("idx:{subtitle} "))
            } else {
                self.double(&format!("track-list/{i}/ff-index")) as i64 == subtitle
            };
            if matches && let Some(id) = self.string(&format!("track-list/{i}/id")) {
                self.set("sid", &id);
                return;
            }
        }
    }

    /// As ExoView's snapshot: `position`, `duration`, `ready`, `buffering`, `ended`, `error` and
    /// `cues` (none: mpv draws the subtitles).
    fn state(&self) -> JsonValue {
        let error = self.failed.borrow().clone().unwrap_or_default();
        if !self.loaded.get() {
            return json!({ "error": error });
        }
        let (position, duration) = (self.double("time-pos") * 1000.0, self.double("duration") * 1000.0);
        let ended = self.flag("eof-reached");
        let ready = error.is_empty() && (duration > 0.0 || position > 0.0);
        json!({
            "position": position,
            "duration": duration,
            "ready": ready,
            "buffering": error.is_empty() && !ended && (!ready || self.flag("paused-for-cache")),
            "ended": ended,
            "error": error,
            "cues": "",
        })
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // SAFETY: the render context goes before the handle it belongs to.
        unsafe {
            let render = self.render.get();
            if !render.is_null() {
                (self.api.render_free)(render);
            }
            (self.api.terminate_destroy)(self.mpv);
        }
    }
}

pub fn register(components: &Components) {
    components.register(NAME, Rc::new(|context: ComponentContext| -> Box<dyn NativeComponent> { Box::new(Mpv::new(context)) }));
}

struct Mpv {
    area: gtk::GLArea,
    player: Option<Rc<Player>>,
    rate: f64,
    volume: f64,
    muted: bool,
}

impl Mpv {
    fn new(context: ComponentContext) -> Self {
        let area = gtk::GLArea::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.set_auto_render(false);
        // It only shows: the keys and the clicks go to the player around it (its controls, its keys).
        area.set_focusable(false);
        area.set_can_focus(false);
        area.set_can_target(false);
        let Some(player) = api().and_then(Player::new).map(Rc::new) else {
            context.emit("state", json!({ "error": "This system has no video player: install mpv's library (libmpv2)" }));
            return Self { area, player: None, rate: 1.0, volume: 1.0, muted: false };
        };
        // The render context once the area has its OpenGL context; mpv draws into the area's
        // framebuffer when it renders.
        {
            let player = player.clone();
            area.connect_realize(move |area| {
                // Once: GTK realizes the area again when it is moved, and mpv has one renderer.
                if !player.render.get().is_null() {
                    return;
                }
                area.make_current();
                if area.error().is_some() {
                    log::error!("no OpenGL for the video");
                    return;
                }
                let mut init = OpenGlInitParams { get_proc_address, get_proc_address_ctx: std::ptr::null_mut() };
                let api_type = c("opengl");
                let mut params = [
                    RenderParam { kind: RENDER_PARAM_API_TYPE, data: api_type.as_ptr() as *mut c_void },
                    RenderParam { kind: RENDER_PARAM_OPENGL_INIT_PARAMS, data: (&mut init as *mut OpenGlInitParams).cast() },
                    RenderParam { kind: RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
                ];
                let mut render = std::ptr::null_mut();
                // SAFETY: the parameters live through the call, as libmpv's render.h says.
                let made = unsafe { (player.api.render_create)(&mut render, player.mpv, params.as_mut_ptr()) };
                if made < 0 {
                    log::error!("mpv's OpenGL renderer did not start ({made})");
                    return;
                }
                player.render.set(render);
                // SAFETY: the flag lives as long as the player, which frees the context first.
                unsafe { (player.api.render_set_update_callback)(render, Some(on_update), Arc::as_ptr(&player.dirty) as *mut c_void) };
            });
        }
        {
            let player = player.clone();
            area.connect_render(move |area, _| {
                let render = player.render.get();
                if render.is_null() {
                    return glib::Propagation::Stop;
                }
                let scale = area.scale_factor();
                let (width, height) = (area.width() * scale, area.height() * scale);
                let mut fbo: c_int = 0;
                // The framebuffer GTK draws the area into (glGetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING)).
                // SAFETY: OpenGL's function, looked up by EGL, on the area's current context.
                unsafe {
                    let get_integer = get_proc_address(std::ptr::null_mut(), c"glGetIntegerv".as_ptr());
                    if !get_integer.is_null() {
                        let get_integer: unsafe extern "C" fn(u32, *mut c_int) = std::mem::transmute(get_integer);
                        get_integer(0x8CA6, &mut fbo);
                    }
                }
                let mut target = OpenGlFbo { fbo, w: width, h: height, internal_format: 0 };
                let mut flip: c_int = 1;
                let mut params = [
                    RenderParam { kind: RENDER_PARAM_OPENGL_FBO, data: (&mut target as *mut OpenGlFbo).cast() },
                    RenderParam { kind: RENDER_PARAM_FLIP_Y, data: (&mut flip as *mut c_int).cast() },
                    RenderParam { kind: RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
                ];
                // SAFETY: the parameters live through the call.
                unsafe { (player.api.render_render)(render, params.as_mut_ptr()) };
                glib::Propagation::Stop
            });
        }
        // Each frame of the window: draw again when mpv asked, and its events and state four
        // times a second.
        {
            let player = player.clone();
            let last = RefCell::new(String::new());
            let ticks = Cell::new(0u32);
            area.add_tick_callback(move |area, _| {
                if player.dirty.swap(false, Ordering::Relaxed) {
                    area.queue_render();
                }
                ticks.set(ticks.get().wrapping_add(1));
                if ticks.get() % 15 == 0 {
                    player.drain();
                    let state = player.state();
                    let text = state.to_string();
                    if *last.borrow() != text {
                        context.emit("state", state);
                        *last.borrow_mut() = text;
                    }
                }
                glib::ControlFlow::Continue
            });
        }
        Self { area, player: Some(player), rate: 1.0, volume: 1.0, muted: false }
    }
}

impl NativeComponent for Mpv {
    fn widget(&self) -> gtk::Widget {
        self.area.clone().upcast()
    }

    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        let Some(player) = self.player.clone() else { return };
        match name {
            // What to play, as `ExoView.load` reads it: the stream, where to start, and the WebVTT
            // subtitles the server makes (the file's own mpv reads itself).
            "source" => {
                let Some(source) = value else { return };
                let url = source.get("url").and_then(JsonValue::as_str).unwrap_or_default();
                let start = source.get("startMs").and_then(JsonValue::as_f64).unwrap_or(0.0);
                *player.pending_subtitles.borrow_mut() = source
                    .get("subtitles")
                    .and_then(JsonValue::as_array)
                    .map(|list| {
                        list.iter()
                            .map(|s| {
                                let field = |key: &str| s.get(key).and_then(JsonValue::as_str).unwrap_or_default().to_owned();
                                (s.get("index").and_then(JsonValue::as_i64).unwrap_or(-1), field("label"), field("language"), field("url"))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                *player.failed.borrow_mut() = None;
                player.file_loaded.set(false);
                player.set("volume", &format!("{}", (self.volume * 100.0).round()));
                player.set("mute", if self.muted { "yes" } else { "no" });
                player.set("speed", &self.rate.to_string());
                // Where to start, for the next file (a property, as `loadfile`'s arguments differ
                // between mpv's versions); none for a live stream, whose start may not be in its
                // playlist yet.
                player.set("start", &if start > 0.0 { format!("{}", start / 1000.0) } else { "none".to_owned() });
                player.command(&["loadfile", url, "replace"]);
                player.loaded.set(true);
            }
            "paused" => player.set("pause", if value.and_then(JsonValue::as_bool).unwrap_or(false) { "yes" } else { "no" }),
            "subtitle" => {
                player.wanted_subtitle.set(value.and_then(JsonValue::as_i64).unwrap_or(-1));
                player.apply_tracks();
            }
            "audio" => {
                player.wanted_audio.set(value.and_then(JsonValue::as_i64).unwrap_or(-1));
                player.apply_tracks();
            }
            "rate" => {
                self.rate = value.and_then(JsonValue::as_f64).unwrap_or(1.0).clamp(0.25, 4.0);
                player.set("speed", &self.rate.to_string());
            }
            "volume" => {
                self.volume = value.and_then(JsonValue::as_f64).unwrap_or(1.0).clamp(0.0, 1.0);
                player.set("volume", &format!("{}", (self.volume * 100.0).round()));
            }
            "muted" => {
                self.muted = value.and_then(JsonValue::as_bool).unwrap_or(false);
                player.set("mute", if self.muted { "yes" } else { "no" });
            }
            _ => {}
        }
    }

    fn command(&mut self, name: &str, args: &JsonValue) {
        let Some(player) = &self.player else { return };
        if name == "seek" {
            let ms = args.get("ms").and_then(JsonValue::as_f64).unwrap_or(0.0);
            player.command(&["seek", &format!("{}", ms / 1000.0), "absolute"]);
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        if let Some(player) = &self.player {
            player.command(&["stop"]);
        }
    }
}

/// The app's window into full screen, or out of it (the player's button and `f`): the window the
/// video is in, or else the app's first. The keys come on NSH's engine thread, and GTK is the main
/// thread's: it is done there.
pub fn toggle_fullscreen() {
    glib::MainContext::default().invoke(|| {
        let windows = gtk::Window::list_toplevels();
        let window = windows
            .iter()
            .filter_map(|window| window.clone().downcast::<gtk::Window>().ok())
            .find(|window| window.is_active())
            .or_else(|| windows.iter().find_map(|window| window.clone().downcast::<gtk::Window>().ok()));
        let Some(window) = window else {
            log::error!("full screen: no window");
            return;
        };
        if window.is_fullscreen() {
            window.unfullscreen();
        } else {
            window.fullscreen();
        }
    });
}
