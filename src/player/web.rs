//! The video player on the web (Remux's `player/web.rs`): a `<video>` of the app's own, put into
//! the player's view (found by its test id, which NSH's page writes as `data-testid`), since a
//! native component stays empty in a browser. Direct streams play as they are; HLS through Shaka
//! (loaded from cdnjs as Remux does), or the browser's own where Shaka can't (an iPhone). Text
//! subtitles are the browser's `<track>`s, fetched here first so a stream of another host works.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{HtmlTrackElement, HtmlVideoElement, TextTrackMode};

use super::engine::SubtitleTrack;
use super::shaka;

/// The test id of the view the `<video>` goes into.
pub const HOST: &str = "remux-video-host";

/// How it plays, as the player reads it.
pub struct Snapshot {
    pub position_ms: f64,
    pub duration_ms: f64,
    pub paused: bool,
    pub ended: bool,
    /// Whether it has what it needs to play on (`readyState` of at least HAVE_FUTURE_DATA).
    pub ready: bool,
    /// The browser would not start it without a press (an iPhone, with sound): it waits paused.
    pub blocked: bool,
    pub error: Option<String>,
}

/// Whether the browser's own HLS is the one to use: Safari's, and every iPhone browser's (they
/// are all Safari underneath). It plays what the server makes, and keeps the buffer where the
/// system can let go of it; Shaka there holds the stream in the page, which a phone runs out of
/// memory with. Chrome says it plays HLS too, but refuses some of what the server makes.
fn native_hls(video: &HtmlVideoElement) -> bool {
    let agent = web_sys::window().and_then(|window| window.navigator().user_agent().ok()).unwrap_or_default();
    !video.can_play_type("application/vnd.apple.mpegurl").is_empty() && !agent.contains("Chrome") && !agent.contains("Android")
}

pub struct WebVideo {
    video: HtmlVideoElement,
    shaka: Rc<RefCell<Option<shaka::Player>>>,
    tracks: RefCell<Vec<(i64, HtmlTrackElement)>>,
    blobs: Rc<RefCell<Vec<String>>>,
    error: Rc<RefCell<Option<String>>>,
    /// Counts the streams loaded, so a late answer for an old one is dropped.
    generation: Rc<Cell<u64>>,
    blocked: Rc<Cell<bool>>,
}

/// Starts `video`, and notes it when the browser wants a press first.
fn start(video: &HtmlVideoElement, blocked: &Rc<Cell<bool>>) {
    if let Ok(promise) = video.play() {
        let blocked = blocked.clone();
        spawn_local(async move {
            // A play() that is interrupted rejects too; only "not allowed" waits for a press.
            if let Err(error) = JsFuture::from(promise).await {
                let name = js_sys::Reflect::get(&error, &"name".into()).ok().and_then(|name| name.as_string()).unwrap_or_default();
                if name == "NotAllowedError" {
                    blocked.set(true);
                }
            }
        });
    }
}

/// The text at `url` as a `blob:` address of this page.
async fn fetch_as_blob_url(url: &str) -> Result<String, String> {
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(url)).await.map_err(|e| format!("{e:?}"))?.unchecked_into();
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    let text = JsFuture::from(response.text().map_err(|e| format!("{e:?}"))?).await.map_err(|e| format!("{e:?}"))?;
    let parts = js_sys::Array::of1(&text);
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("text/vtt");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options).map_err(|e| format!("{e:?}"))?;
    web_sys::Url::create_object_url_with_blob(&blob).map_err(|e| format!("{e:?}"))
}

impl WebVideo {
    /// A `<video>` in the view with the test id [`HOST`], once the page has it.
    pub fn attach() -> Option<Self> {
        let document = web_sys::window()?.document()?;
        let host = document.query_selector(&format!("[data-testid=\"{HOST}\"]")).ok()??;
        let video: HtmlVideoElement = document.create_element("video").ok()?.unchecked_into();
        let style = video.style();
        for (name, value) in [("position", "absolute"), ("inset", "0"), ("width", "100%"), ("height", "100%"), ("object-fit", "contain"), ("background", "black")] {
            let _ = style.set_property(name, value);
        }
        video.set_attribute("playsinline", "").ok();
        host.append_child(&video).ok()?;
        Some(Self {
            video,
            shaka: Rc::default(),
            tracks: RefCell::default(),
            blobs: Rc::default(),
            error: Rc::default(),
            generation: Rc::default(),
            blocked: Rc::default(),
        })
    }

    fn add_subtitles(&self, subtitles: &[SubtitleTrack], shown: Option<i64>) {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else { return };
        let mut tracks = self.tracks.borrow_mut();
        for subtitle in subtitles.iter().filter(|subtitle| !subtitle.url.is_empty()) {
            let Ok(element) = document.create_element("track") else { continue };
            let element: HtmlTrackElement = element.unchecked_into();
            element.set_kind("subtitles");
            element.set_label(&subtitle.label);
            if let Some(language) = &subtitle.language {
                element.set_srclang(language);
            }
            let _ = self.video.append_child(&element);
            // A browser reads a track of another origin only when the video asks for CORS,
            // which a direct stream redirected to a host without it would not survive.
            let (url, track_element, blobs) = (subtitle.url.clone(), element.clone(), self.blobs.clone());
            spawn_local(async move {
                match fetch_as_blob_url(&url).await {
                    Ok(local) => {
                        track_element.set_src(&local);
                        blobs.borrow_mut().push(local);
                    }
                    // The address without its query, which has the key to the server.
                    Err(error) => log::warn!("subtitle {}: {error}", url.split('?').next().unwrap_or_default()),
                }
            });
            if let Some(track) = element.track() {
                track.set_mode(if shown == Some(subtitle.index) { TextTrackMode::Showing } else { TextTrackMode::Hidden });
            }
            tracks.push((subtitle.index, element));
        }
    }

    fn play(&self) {
        self.blocked.set(false);
        start(&self.video, &self.blocked);
    }

    /// Plays `url` from `start_ms`.
    pub fn load(&self, url: String, hls: bool, start_ms: f64, subtitles: &[SubtitleTrack], shown: Option<i64>) {
        *self.error.borrow_mut() = None;
        self.add_subtitles(subtitles, shown);
        if !hls {
            // The start as a media fragment: the browser goes there itself.
            self.video.set_src(&format!("{url}#t={:.3}", start_ms / 1000.0));
            self.play();
            return;
        }
        if native_hls(&self.video) {
            self.video.set_src(&format!("{url}#t={:.3}", start_ms / 1000.0));
            self.play();
            return;
        }
        let native = !self.video.can_play_type("application/vnd.apple.mpegurl").is_empty();
        let blocked = self.blocked.clone();
        let (video, shaka_slot, error, generation) = (self.video.clone(), self.shaka.clone(), self.error.clone(), self.generation.clone());
        let this_generation = generation.get();
        spawn_local(async move {
            // Shaka plays more of what the server makes than a browser's own HLS does (Chrome
            // refuses some of it); the browser's is for where there is no Media Source.
            if shaka::ensure_loaded().await && shaka::is_browser_supported() {
                shaka::install_polyfills();
                let player = shaka::Player::new();
                let failure = error.clone();
                let on_error = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
                    let detail = js_sys::Reflect::get(&event, &"detail".into()).unwrap_or(JsValue::UNDEFINED);
                    let code = js_sys::Reflect::get(&detail, &"code".into()).ok().and_then(|code| code.as_f64());
                    log::error!("shaka: {detail:?}");
                    *failure.borrow_mut() = Some(format!("The stream could not be played{}", code.map(|code| format!(" (Shaka error {code})")).unwrap_or_default()));
                });
                player.add_event_listener("error", &on_error);
                on_error.forget();
                *shaka_slot.borrow_mut() = Some(player.clone().unchecked_into());
                let start_at = if start_ms > 0.0 { JsValue::from_f64(start_ms / 1000.0) } else { JsValue::UNDEFINED };
                let result = async {
                    player.attach(&video).await?;
                    player.load(&url, start_at).await
                }
                .await;
                if generation.get() != this_generation {
                    return;
                }
                match result {
                    Ok(_) => start(&video, &blocked),
                    Err(reason) => {
                        log::error!("shaka could not load the stream: {reason:?}");
                        *error.borrow_mut() = Some("The stream could not be loaded".to_owned());
                    }
                }
            } else if native {
                video.set_src(&format!("{url}#t={:.3}", start_ms / 1000.0));
                start(&video, &blocked);
            } else {
                *error.borrow_mut() = Some("This browser cannot play HLS".to_owned());
            }
        });
    }

    pub fn set_paused(&self, paused: bool) {
        if paused {
            let _ = self.video.pause();
        } else {
            self.play();
        }
    }

    pub fn seek(&self, ms: f64) {
        self.video.set_current_time(ms / 1000.0);
    }

    pub fn set_subtitle(&self, index: Option<i64>) {
        for (track_index, element) in self.tracks.borrow().iter() {
            if let Some(track) = element.track() {
                track.set_mode(if Some(*track_index) == index { TextTrackMode::Showing } else { TextTrackMode::Hidden });
            }
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let finite = |value: f64| if value.is_finite() { value } else { 0.0 };
        let media_error = self.video.error().map(|error| {
            let reason = match error.code() {
                web_sys::MediaError::MEDIA_ERR_NETWORK => "network error",
                web_sys::MediaError::MEDIA_ERR_DECODE => "could not be decoded",
                web_sys::MediaError::MEDIA_ERR_SRC_NOT_SUPPORTED => "this browser cannot play this version",
                _ => "unknown error",
            };
            format!("{reason} ({})", error.message())
        });
        Snapshot {
            position_ms: finite(self.video.current_time()) * 1000.0,
            duration_ms: finite(self.video.duration()) * 1000.0,
            paused: self.video.paused(),
            ended: self.video.ended(),
            ready: self.video.ready_state() >= 3,
            blocked: self.blocked.get(),
            error: self.error.borrow().clone().or(media_error),
        }
    }

    /// Stops, lets go of the stream (browsers keep it in their media controls otherwise) and takes
    /// the `<video>` away.
    pub fn close(&self) {
        self.generation.set(self.generation.get() + 1);
        if let Some(player) = self.shaka.borrow_mut().take() {
            spawn_local(async move {
                let _ = player.destroy().await;
            });
        }
        for blob in self.blobs.borrow_mut().drain(..) {
            let _ = web_sys::Url::revoke_object_url(&blob);
        }
        let _ = self.video.pause();
        self.video.remove_attribute("src").ok();
        self.video.load();
        self.video.remove();
    }
}
