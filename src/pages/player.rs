//! The video player, over the whole app (Remux's `components/player`): the stream the server
//! says, played by ExoPlayer (the native component `remux.exo`, `player/exo.rs`), on the web by a
//! `<video>` (`player/web.rs`), or by a Chromecast; Remux's controls over it, and what the server
//! is told (start, progress, stop), so Continue Watching and Next Up follow.
//!
//! On a TV, with the controls hidden: OK plays and pauses, left and right go 10 seconds (a channel
//! has no time to go along), up and down show the controls, or go to "Up next" or "Skip intro"
//! when one shows; with the controls showing the remote goes between them, and the seek bar is a
//! stop of its own. Back closes. On the web Remux's keys: space or k, j and l, the arrows, m, f, n,
//! c and the digits.

use std::rc::Rc;
use std::time::Duration;

use nsh_base::{SliderValue, button, slider, theme};
use nsh_core::keyboard::KeyEvent;
use nsh_core::prelude::*;
use nsh_icon::labeled_icon;
use nsh_icon_phosphor::Phosphor;
use serde_json::json;

use crate::capabilities::Capabilities;
use crate::hooks::server;
use crate::media::{Media, MediaSource, MediaType};
use crate::player::api::{self as player_api, Stage};
use crate::player::engine::StreamKind;
use crate::player::reporter::Reporter;
use crate::player::stream::{self, ResolvedStream, SegmentKind};
use crate::ui::{self, tokens::{is_tv, motion, scrim, track}};

/// What plays, if anything.
#[derive(Clone, Default, PartialEq)]
struct Playing(Option<Request>);

/// What to play: the title, and what of it. Made again (from where it was) for what the player
/// itself cannot change: another audio track of a stream the server makes, a subtitle the server
/// draws into the picture, another version, a retry.
#[derive(Clone, PartialEq)]
struct Request {
    media: Media,
    /// The version chosen; else the server's first.
    source: Option<MediaSource>,
    audio: Option<i64>,
    /// A picture subtitle the server draws into the video.
    burned: Option<i64>,
    /// The subtitle to show from the start (`Some(None)`: none), else the stream's.
    subtitle: Option<Option<i64>>,
    start_ms: Option<f64>,
}

impl Request {
    fn of(media: Media) -> Self {
        Self { media, source: None, audio: None, burned: None, subtitle: None, start_ms: None }
    }

    /// This again, from `at` (ms).
    fn from(&self, at: f64) -> Self {
        Self { start_ms: Some(at), ..self.clone() }
    }
}

fn start(request: Request) {
    // Music and video do not play together.
    crate::music::pause();
    global::<Playing>().set(Playing(Some(request)));
}

/// Plays `media` from where it was left.
pub fn play(media: Media) {
    start(Request::of(media));
}

/// Plays this version of `media` from where it was left.
pub fn play_source(media: Media, source: MediaSource) {
    start(Request { source: Some(source), ..Request::of(media) });
}

/// Closes the player when it is open (music starts).
pub fn close() {
    if global::<Playing>().with_untracked(|playing| playing.0.is_some()) {
        global::<Playing>().set(Playing(None));
    }
}

fn stop() {
    global::<Playing>().set(Playing(None));
    // The remote goes back to where playing started: the play button of the page over the
    // place, or of the place's first page (`Scope::top` is the page made last: on Home that is
    // another place's).
    if is_tv() {
        if crate::nav::can_go_back() {
            crate::tv_focus::Scope::top().to_play();
        } else {
            crate::tv_focus::to_first(crate::nav::place_untracked());
        }
    }
}

/// "1:02:03" or "2:03".
fn clock(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as i64;
    let (hours, minutes, rest) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 { format!("{hours}:{minutes:02}:{rest:02}") } else { format!("{minutes}:{rest:02}") }
}

/// The speeds to play at (Remux's).
const SPEEDS: [f64; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];
/// "Up next" shows this long before the end, or from the credits on.
const UP_NEXT_SECONDS: f64 = 30.0;
/// A channel that stops is started again this often in a row, then it is given up on.
const LIVE_RETRIES: u32 = 3;

/// The player, once for the app: in the shell, over everything.
pub fn player_host() -> impl IntoElement {
    let playing = global::<Playing>();
    // Back closes the player, before anything under it.
    on_back_press(move || {
        if playing.with_untracked(|playing| playing.0.is_none()) {
            return false;
        }
        stop();
        true
    });
    dynamic(move || match playing.with(|playing| playing.0.clone()) {
        Some(request) => untracked(move || player(request).into_element()),
        None => view().into_element(),
    })
}

fn player(request: Request) -> impl IntoElement {
    let item_id = request.media.id;
    // How fast the line is, for Automatic quality, when the last measurement is old.
    crate::profile::measure_if_stale();
    // Where it plays: here, or on the Chromecast cast to, which has a session of its own on the
    // server (being made: wait for it). Picking another plays it there from where it was.
    // (A memo: the list of devices changes now and then, which is no reason to ask again.)
    let target = computed(move || {
        let device = crate::cast::active();
        crate::cast::playback_server().map(|_| device)
    });
    // The page's own request: its versions, which the stream is asked for, by what plays it.
    let asked = request.clone();
    let stream = resource(move || target.get(), move |device: Option<crate::cast::Device>, _| {
        let asked = asked.clone();
        async move {
            let server = untracked(crate::cast::playback_server).unwrap_or_else(server);
            let item = server.media_details(item_id).await.map_err(|error| error.to_string())?.ok_or("not found")?;
            // The version chosen (a version's id is its file's, the same to every session); else
            // the server's first.
            let source = asked.source.clone().or_else(|| item.media_sources.first().cloned());
            let capabilities = match &device {
                Some(device) => Capabilities::chromecast(&device.model),
                None => Capabilities::detect(),
            };
            let resolved = stream::resolve(&server, &item, source.as_ref(), &capabilities, asked.audio, asked.burned)
                .await
                .map_err(|error| error.to_string())?;
            Ok::<_, String>((item, Rc::new(resolved), device, server))
        }
    });
    // Where it was, when the place it plays changes.
    let resume_at = stored(request.start_ms);
    let failed_request = request.clone();
    view()
        .absolute()
        .inset_0()
        .bg(theme().background)
        .child(dynamic(move || match (stream.get(), stream.error()) {
            (Some((item, resolved, device, playing_server)), _) => {
                let request = Request { start_ms: resume_at.with(|at| *at), ..request.clone() };
                untracked(move || playback(item, resolved, request, device, playing_server, resume_at).into_element())
            }
            (_, Some(error)) => failure(format!("Could not play it: {error}"), failed_request.clone()).into_element(),
            _ => view().flex_1().items_center().justify_center().child(ui::spinner()).into_element(),
        }))
}

/// What shows when it cannot play: why, a way to try again (from where it was), and a way out.
fn failure(message: impl IntoReactive<SharedString>, retry: Request) -> impl IntoElement {
    view()
        .absolute()
        .inset_0()
        .items_center()
        .justify_center()
        .gap_4()
        .p_6()
        .bg(theme().background.opacity(0.8))
        .child(ui::empty(message))
        .child(
            view()
                .flex_row()
                .gap_3()
                .child(ui::text_button(ui::Variant::Primary, ui::Size::Regular, Phosphor::ArrowClockwise, "Retry", move || start(retry.clone())))
                .child(ui::text_button(ui::Variant::Secondary, ui::Size::Regular, Phosphor::X, "Close", stop)),
        )
}

fn playback(
    item: Media,
    resolved: Rc<ResolvedStream>,
    request: Request,
    device: Option<crate::cast::Device>,
    playing_server: std::sync::Arc<crate::server::JellyfinServer>,
    resume_at: Stored<Option<f64>>,
) -> impl IntoElement {
    let start_ms = request.start_ms.unwrap_or_else(|| item.resume_ms());
    let live = item.is_live();
    // The Chromecast, when it plays there (the session with it ends with the player).
    let casting = device.clone();
    let cast_engine = stored(device.as_ref().map(|device| Rc::new(crate::cast::CastEngine::connect(device))));
    let video_ref = element_ref();
    let paused = signal(false);
    let position = signal(0.0f64);
    let duration = signal(resolved.duration_ms / 1000.0);
    let buffering = signal(true);
    let controls = signal(true);
    // The last time something was done: the controls go after a while without.
    let touched = stored(0u32);
    let cues = signal(String::new());
    let failed = signal(None::<String>);
    let started = signal(false);
    let ended = signal(false);
    let seeking = signal(false);
    let rate = signal(1.0f64);
    // The web's: a phone and a TV have keys for the volume.
    let volume = signal(1.0f64);
    let muted = signal(false);
    // The subtitle shown: the one asked for, else the stream's (one drawn into the picture is
    // the stream's too).
    let subtitle = signal(request.subtitle.unwrap_or(resolved.subtitle));
    // What to play, as `ExoView.load` reads it: the stream, where to start, the WebVTT subtitles the
    // server makes, and the tracks of the file in their order.
    let source = json!({
        "url": resolved.url,
        "startMs": start_ms,
        "hls": resolved.kind == StreamKind::Hls,
        "subtitles": resolved.subtitle_tracks.iter().filter(|track| !track.url.is_empty()).map(|track| json!({
            "index": track.index, "label": track.label, "language": track.language, "url": track.url,
        })).collect::<Vec<_>>(),
        "audioOrder": resolved.audio_tracks.iter().map(|track| track.index).collect::<Vec<_>>(),
        "fileSubtitles": resolved.subtitle_tracks.iter().filter(|track| track.url.is_empty() && !track.burned).map(|track| json!([track.index, track.codec])).collect::<Vec<_>>(),
    });
    // Every subtitle, the ones the server has to draw into the picture too (they take another
    // stream).
    let subtitle_tracks: Vec<(i64, String, bool)> = resolved.subtitle_tracks.iter().map(|track| (track.index, track.label.clone(), track.burned)).collect();
    let subtitles_open = signal(false);
    let audio_open = signal(false);
    let options_open = signal(false);
    // The audio tracks, and the one that plays: the player picks one of the file it plays as it is;
    // for another, the server makes another stream, which plays on from where this one was.
    let audio_tracks: Vec<(i64, String, String)> = resolved.audio_tracks.iter().map(|track| (track.index, track.label.clone(), track.codec.clone())).collect();
    let current_audio = signal(resolved.audio);
    let direct = resolved.kind == StreamKind::Direct;
    // This, again, from where it is now: for what only another stream can do.
    let again = {
        let request = request.clone();
        move |change: &dyn Fn(&mut Request)| {
            let mut next = request.from(position.get_untracked() * 1000.0);
            change(&mut next);
            start(next);
        }
    };
    let pick_audio = {
        let again = again.clone();
        move |index: i64, codec: String| {
            audio_open.set(false);
            if direct && Capabilities::detect().player_picks_audio(&codec) {
                current_audio.set(Some(index));
                return;
            }
            again(&|next| next.audio = Some(index));
        }
    };
    // A subtitle: one the player shows, or one the server draws into the picture (another
    // stream; and to leave one of those, another stream again).
    let burned_now = request.burned;
    let pick_subtitle = {
        let again = again.clone();
        move |index: Option<i64>, burned: bool| {
            subtitles_open.set(false);
            match (burned, burned_now) {
                (true, _) => again(&|next| {
                    next.burned = index;
                    next.subtitle = None;
                }),
                (false, Some(_)) => again(&|next| {
                    next.burned = None;
                    next.subtitle = Some(index);
                }),
                (false, None) => subtitle.set(index),
            }
        }
    };
    let subtitle_tracks_sheet = subtitle_tracks.clone();
    let has_audio_choice = audio_tracks.len() > 1;
    let has_subtitles = !subtitle_tracks.is_empty();
    // Where it was last, outside the player's state: the report as the player goes is made when
    // that state is gone.
    let last_at = Rc::new(std::cell::Cell::new(start_ms));
    let reporter = Rc::new(
        Reporter::new(playing_server, resolved.item_id, resolved.source_id, resolved.play_session_id.clone(), resolved.method.clone())
            .with_parents([item.season_id, item.series_id]),
    );
    // What the server is told, with the audio and the subtitle that play (its "Now playing").
    let report = {
        let reporter = reporter.clone();
        move |stage: Stage| {
            let reporter = reporter.clone();
            let (at, still) = (position.get_untracked() * 1000.0, paused.get_untracked());
            let (audio, shown) = (current_audio.get_untracked(), subtitle.get_untracked());
            spawn(async move { reporter.report(stage, at, still, audio, shown).await });
        }
    };
    // Progress every ten seconds while it plays; stopped when the player goes.
    {
        let report = report.clone();
        set_interval(Duration::from_secs(10), move || {
            if !paused.get_untracked() {
                report(Stage::Progress);
            }
        });
    }
    {
        let reporter = reporter.clone();
        let last_at = last_at.clone();
        cleanup(move || {
            let reporter = reporter.clone();
            let at = last_at.get();
            spawn(async move { reporter.report(Stage::Stopped, at, true, None, None).await });
        });
    }
    // The buttons the remote goes to: the play button when the controls show, the corner's button
    // ("Up next", "Skip intro").
    let root = element_ref();
    let play_button = element_ref();
    let corner_button = element_ref();
    let (close_button, audio_button, subtitles_button, options_button) = (element_ref(), element_ref(), element_ref(), element_ref());
    let (back_button, forward_button, seek_stop) = (element_ref(), element_ref(), element_ref());
    // Where a TV's remote is among the controls: the row (0 the top, 1 play, 2 the seek bar) and
    // the place in it; and whether it is on the corner ("Up next", "Skip intro").
    let at = signal((1usize, 1usize));
    let on_corner = signal(false);
    let show_controls = move || {
        let was = controls.get_untracked();
        controls.set(true);
        if !was && is_tv() {
            at.set((1, 1));
            on_corner.set(false);
            set_timeout(Duration::from_millis(60), move || play_button.focus());
        }
        let this = touched.with(|touched| {
            *touched += 1;
            *touched
        });
        set_timeout(Duration::from_secs(5), move || {
            if touched.with(|touched| *touched) == this && !paused.get_untracked() && !options_open.get_untracked() {
                controls.set(false);
                // The remote's focus was in the controls, which are gone: back to the picture.
                if is_tv() {
                    root.focus();
                }
            }
        });
    };
    // The `<video>` of the web, once it is in the page (`player/web.rs`).
    #[cfg(target_arch = "wasm32")]
    let web_video = stored(None::<Rc<crate::player::web::WebVideo>>);
    // To a place in the stream (ms), on what plays it.
    let seek_to = move |ms: f64| match cast_engine.with(|engine| engine.clone()) {
        Some(engine) => engine.seek(ms),
        #[cfg(target_arch = "wasm32")]
        None => {
            if let Some(video) = web_video.with(|video| video.clone()) {
                video.seek(ms);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        None => video_ref.command("seek", json!({ "ms": ms })),
    };
    // Where it is, for when it goes on elsewhere, and for the report as it stops.
    {
        let last_at = last_at.clone();
        effect(move || {
            let at = position.get() * 1000.0;
            if at > 0.0 {
                last_at.set(at);
                untracked(|| resume_at.with(|resume| *resume = Some(at)));
            }
        });
    }
    let seek_by = move |seconds: f64| {
        if live {
            return;
        }
        let to = (position.get_untracked() + seconds).clamp(0.0, duration.get_untracked());
        position.set(to);
        seek_to(to * 1000.0);
        show_controls();
    };
    let toggle = {
        let report = report.clone();
        move || {
            paused.update(|paused| *paused = !*paused);
            report(Stage::Progress);
            show_controls();
        }
    };

    // A channel: the ones before and after it, to go to (Remux's zapping).
    let channels = resource(
        move || live.then_some(()),
        |(), _| async {
            let query = crate::server::MediaQuery::builder().types(vec![MediaType::Channel]).limit(500).build();
            server().media(&query).await.map_err(|error| error.to_string())
        },
    );
    let channel_id = item.id;
    let zap = move |step: i64| {
        let Some(list) = untracked(|| channels.get()) else { return };
        let Some(at) = list.iter().position(|channel| channel.id == channel_id) else { return };
        let to = at as i64 + step;
        if to >= 0 && (to as usize) < list.len() {
            play(list[to as usize].clone());
        }
    };
    let can_zap = move |step: i64| {
        channels.get().is_some_and(|list| {
            list.iter().position(|channel| channel.id == channel_id).is_some_and(|at| {
                let to = at as i64 + step;
                to >= 0 && (to as usize) < list.len()
            })
        })
    };

    // The episode after this one, for "Up next" and for the end.
    let next = resource(
        {
            let is_episode = item.media_type == MediaType::Episode;
            move || is_episode.then_some(())
        },
        {
            let item = item.clone();
            move |(), _| {
                let item = item.clone();
                async move { Ok::<_, String>(player_api::next_episode(&server(), &item).await.ok().flatten()) }
            }
        },
    );
    // The segment the position is in, to skip (an intro, a recap, the credits); and whether "Up
    // next" shows: near the end, or from the credits on (the skip of the credits gives way to it).
    let segments = resolved.segments.clone();
    let in_outro = {
        let segments = segments.clone();
        computed(move || {
            let at = position.get() * 1000.0;
            segments.iter().any(|segment| segment.kind == SegmentKind::Outro && at >= segment.start_ms)
        })
    };
    let show_next = computed(move || {
        let near_end = duration.get() > 0.0 && duration.get() - position.get() <= UP_NEXT_SECONDS;
        started.get() && !ended.get() && next.get().flatten().is_some() && (in_outro.get() || near_end)
    });
    let skippable = computed(move || {
        let at = position.get() * 1000.0;
        segments
            .iter()
            .filter(|segment| !(show_next.get() && segment.kind == SegmentKind::Outro))
            .find(|segment| at >= segment.start_ms && at < segment.end_ms - 1000.0)
            .map(|segment| (segment.kind, segment.end_ms))
    });
    let corner = move || show_next.get() || skippable.get().is_some();
    let title = match (&item.media_type, &item.series_name) {
        (MediaType::Episode, Some(show)) => format!("{show} · {}", item.title),
        _ => item.title.clone(),
    };

    // How it plays, from ExoPlayer's snapshot, the web's `<video>` or the Chromecast's.
    let live_retries = stored(0u32);
    let on_state = {
        let request = request.clone();
        Rc::new(move |state: &serde_json::Value| {
            let number = |key: &str| state.get(key).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
            let flag = |key: &str| state.get(key).and_then(serde_json::Value::as_bool).unwrap_or(false);
            cues.set(state.get("cues").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned());
            if let Some(error) = state.get("error").and_then(serde_json::Value::as_str).filter(|error| !error.is_empty()) {
                if failed.get_untracked().is_some() {
                    return;
                }
                // A device that says it decodes Dolby audio and then fails on it: asked again,
                // once, with the audio made into something it plays (Remux).
                let dolby = cfg!(target_os = "android")
                    && error.contains("MediaCodecAudioRenderer")
                    && (error.contains("audio/eac3") || error.contains("audio/ac3"));
                if dolby && !crate::capabilities::DOLBY_FAILED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    log::error!("this device fails on Dolby audio, asking for it transcoded: {error}");
                    start(request.from(position.get_untracked() * 1000.0));
                    return;
                }
                // A channel that stops is started again (the server ends a stream when its
                // source has a hiccup).
                if live && live_retries.with(|retries| *retries) < LIVE_RETRIES {
                    live_retries.with(|retries| *retries += 1);
                    log::error!("live channel stopped, starting it again: {error}");
                    start(request.clone());
                    return;
                }
                failed.set(Some(error.to_owned()));
                return;
            }
            if !flag("ready") && !flag("buffering") && !flag("ended") {
                return;
            }
            // Not while the thumb is held: the player is still where it was.
            if !seeking.get_untracked() {
                position.set(number("position") / 1000.0);
            }
            if number("duration") > 0.0 {
                duration.set(number("duration") / 1000.0);
            }
            buffering.set(flag("buffering"));
            if flag("ready") && !started.get_untracked() {
                started.set(true);
                live_retries.with(|retries| *retries = 0);
                report(Stage::Start);
                show_controls();
            }
            if flag("ended") && !ended.get_untracked() {
                ended.set(true);
                // A channel does not end: its stream did, and it is started again.
                if live {
                    start(request.clone());
                    return;
                }
                // The next episode of a show, else the player closes.
                match untracked(|| next.get()).flatten() {
                    Some(episode) => play(episode),
                    None => stop(),
                }
            }
        })
    };
    // The web: its `<video>` goes into the view drawn for it once the page has that view, and is
    // read four times a second as ExoPlayer's snapshot is.
    #[cfg(target_arch = "wasm32")]
    if cast_engine.with(|engine| engine.is_none()) {
        let (url, hls, tracks, shown) = (resolved.url.clone(), resolved.kind == StreamKind::Hls, resolved.subtitle_tracks.clone(), subtitle.get_untracked());
        let on_state = on_state.clone();
        let closing: Rc<std::cell::RefCell<Option<Rc<crate::player::web::WebVideo>>>> = Rc::default();
        let closing_at_end = closing.clone();
        set_timeout(Duration::from_millis(50), move || {
            let Some(video) = crate::player::web::WebVideo::attach() else {
                log::error!("the player's view is not in the page");
                return;
            };
            video.load(url.clone(), hls, start_ms, &tracks, shown);
            let video = Rc::new(video);
            *closing.borrow_mut() = Some(video.clone());
            web_video.with(|slot| *slot = Some(video));
        });
        set_interval(Duration::from_millis(250), move || {
            let Some(video) = web_video.with(|slot| slot.clone()) else { return };
            let now = video.snapshot();
            // The browser wants a press to start (an iPhone): it waits paused, with its controls.
            if now.blocked && !paused.get_untracked() {
                paused.set(true);
                buffering.set(false);
                controls.set(true);
                return;
            }
            on_state(&json!({
                "position": now.position_ms,
                "duration": now.duration_ms,
                "ready": now.ready,
                "buffering": !now.ready && !now.ended && !now.paused && now.error.is_none(),
                "ended": now.ended,
                "error": now.error.unwrap_or_default(),
            }));
        });
        let web = move || web_video.with(|slot| slot.clone());
        effect(move || {
            let still = paused.get();
            untracked(|| web().map(|video| video.set_paused(still)));
        });
        effect(move || {
            let shown = subtitle.get();
            untracked(|| web().map(|video| video.set_subtitle(shown)));
        });
        effect(move || {
            let speed = rate.get();
            untracked(|| web().map(|video| video.set_rate(speed)));
        });
        effect(move || {
            let (level, silent) = (volume.get(), muted.get());
            untracked(|| {
                web().map(|video| {
                    video.set_volume(level);
                    video.set_muted(silent);
                })
            });
        });
        // The `<video>` goes with the player. Kept outside the player's state for this: that is
        // gone by the time the cleanup runs.
        cleanup(move || {
            if let Some(video) = closing_at_end.borrow_mut().take() {
                video.close();
            }
        });
    }
    if let Some(engine) = cast_engine.with(|engine| engine.clone()) {
        // The stream to the Chromecast, which fetches it itself; the subtitles it shows are the
        // server's WebVTT ones.
        let subtitles = resolved.subtitle_tracks.iter().filter(|track| !track.url.is_empty()).map(|track| crate::cast::Subtitle {
            index: track.index,
            label: track.label.clone(),
            language: track.language.clone(),
            url: track.url.clone(),
        }).collect();
        engine.load(resolved.url.clone(), resolved.kind == StreamKind::Hls, start_ms, subtitles, subtitle.get_untracked());
        let poll = engine.clone();
        let on_state = on_state.clone();
        set_interval(Duration::from_millis(500), move || {
            let now = poll.snapshot();
            let ready = now.playing || (now.duration_ms > 0.0 && !now.ended);
            on_state(&json!({
                "position": now.position_ms,
                "duration": now.duration_ms,
                "ready": ready,
                "buffering": !ready && !now.ended && now.error.is_none(),
                "ended": now.ended,
                "error": now.error.unwrap_or_default(),
            }));
        });
        let (pause_engine, subtitle_engine, rate_engine) = (engine.clone(), engine.clone(), engine);
        effect(move || {
            let still = paused.get();
            untracked(|| pause_engine.set_paused(still));
        });
        effect(move || {
            let shown = subtitle.get();
            untracked(|| subtitle_engine.set_subtitle(shown));
        });
        effect(move || {
            let speed = rate.get();
            untracked(|| rate_engine.set_rate(speed));
        });
    }

    // The keyboard of the web (Remux's keys).
    let next_for_key = next;
    let subtitle_cycle = subtitle_tracks.iter().filter(|(_, _, burned)| !burned).map(|(index, _, _)| *index).collect::<Vec<_>>();
    let web_key = {
        let toggle = toggle.clone();
        move |key: &str| -> bool {
            match key {
                " " | "k" | "K" => toggle(),
                "j" | "J" => seek_by(-10.0),
                "l" | "L" => seek_by(10.0),
                "ArrowLeft" => seek_by(-5.0),
                "ArrowRight" => seek_by(5.0),
                "ArrowUp" => volume.update(|volume| *volume = (*volume + 0.05).min(1.0)),
                "ArrowDown" => volume.update(|volume| *volume = (*volume - 0.05).max(0.0)),
                "m" | "M" => muted.update(|muted| *muted = !*muted),
                "n" | "N" => {
                    if let Some(episode) = untracked(|| next_for_key.get()).flatten() {
                        play(episode);
                    }
                }
                "PageUp" if live => zap(1),
                "PageDown" if live => zap(-1),
                "f" | "F" => {
                    #[cfg(target_arch = "wasm32")]
                    crate::player::web::WebVideo::toggle_fullscreen();
                }
                // The subtitles in turn, then none.
                "c" | "C" => {
                    let now = subtitle.get_untracked();
                    let at = now.and_then(|index| subtitle_cycle.iter().position(|other| *other == index));
                    let then = match at {
                        None => subtitle_cycle.first().copied(),
                        Some(at) => subtitle_cycle.get(at + 1).copied(),
                    };
                    subtitle.set(then);
                }
                digit if !live && digit.len() == 1 && digit.chars().all(|c| c.is_ascii_digit()) => {
                    let tenth: f64 = digit.parse().unwrap_or(0.0);
                    let to = duration.get_untracked() * tenth / 10.0;
                    position.set(to);
                    seek_to(to * 1000.0);
                }
                _ => return false,
            }
            show_controls();
            true
        }
    };

    // The remote's keys (and the web's) come to the player, not to the page under it.
    if is_tv() || cfg!(target_arch = "wasm32") {
        set_timeout(Duration::from_millis(100), move || root.focus());
    }
    view()
        .element_ref(&root)
        .flex_1()
        .bg(theme().background)
        .focusable(is_tv() || cfg!(target_arch = "wasm32"))
        // A TV with the controls hidden: OK plays and pauses, left and right go 10 seconds (a
        // channel: to the one before or after), up and down show the controls or go to the
        // corner. With the controls showing the remote goes between them, as Android finds them.
        .when(is_tv(), {
            let toggle = toggle.clone();
            let request = request.clone();
            move |v| {
                // The rows of the controls, for the remote: what is in each, and what OK does there.
                type Action = Rc<dyn Fn()>;
                let mut top: Vec<(ElementRef, Action)> = vec![(close_button, Rc::new(stop))];
                if has_audio_choice {
                    top.push((audio_button, Rc::new(move || audio_open.set(true))));
                }
                if has_subtitles {
                    top.push((subtitles_button, Rc::new(move || subtitles_open.set(true))));
                }
                top.push((options_button, Rc::new(move || options_open.set(true))));
                let middle: Vec<(ElementRef, Action)> = vec![
                    (back_button, Rc::new(move || if live { zap(-1) } else { seek_by(-10.0) })),
                    (play_button, Rc::new({
                        let toggle = toggle.clone();
                        move || toggle()
                    })),
                    (forward_button, Rc::new(move || if live { zap(1) } else { seek_by(10.0) })),
                ];
                let rows = Rc::new([top, middle]);
                let corner_press = {
                    let request = request.clone();
                    let _ = request;
                    move || {
                        if show_next.get_untracked() {
                            if let Some(episode) = untracked(|| next.get()).flatten() {
                                play(episode);
                            }
                        } else if let Some((_, end)) = skippable.get_untracked() {
                            seek_to(end);
                            position.set(end / 1000.0);
                            on_corner.set(false);
                            root.focus();
                        }
                    }
                };
                let focus_at = {
                    let rows = rows.clone();
                    move |(row, col): (usize, usize)| {
                        at.set((row, col));
                        match row {
                            2 => seek_stop.focus(),
                            row => {
                                if let Some((target, _)) = rows[row].get(col) {
                                    target.focus();
                                }
                            }
                        }
                    }
                };
                v.capture_keys(crate::tv_focus::capture(&["left", "right", "up", "down", "enter"]))
                .on_key_down(move |event: &KeyEvent| {
                    let key = event.key.as_ref();
                    // On the corner: OK is its own; any other key leaves it.
                    if on_corner.get_untracked() {
                        on_corner.set(false);
                        if key == "Enter" {
                            corner_press();
                            return;
                        }
                        root.focus();
                        if !matches!(key, "ArrowUp" | "ArrowDown") {
                            return;
                        }
                    }
                    if !controls.get_untracked() {
                        match key {
                            "ArrowLeft" if live => zap(-1),
                            "ArrowRight" if live => zap(1),
                            "ArrowLeft" => seek_by(-10.0),
                            "ArrowRight" => seek_by(10.0),
                            "Enter" => toggle(),
                            "ArrowUp" | "ArrowDown" if corner() => {
                                on_corner.set(true);
                                corner_button.focus();
                            }
                            _ => show_controls(),
                        }
                        return;
                    }
                    // The controls show: the remote goes between them (Remux's rows).
                    show_controls();
                    let (row, col) = at.get_untracked();
                    let last_row = if live { 1 } else { 2 };
                    match key {
                        "ArrowUp" if row > 0 => {
                            let col = if row == 2 { 1 } else { col.min(rows[row - 1].len().saturating_sub(1)) };
                            focus_at((row - 1, if row == 1 { rows[0].len() - 1 } else { col }));
                        }
                        "ArrowDown" if row < last_row => focus_at((row + 1, if row == 0 { 1 } else { 0 })),
                        "ArrowDown" if corner() => {
                            on_corner.set(true);
                            corner_button.focus();
                        }
                        "ArrowLeft" if row == 2 => seek_by(-10.0),
                        "ArrowRight" if row == 2 => seek_by(10.0),
                        "ArrowLeft" if col > 0 => focus_at((row, col - 1)),
                        "ArrowRight" if col + 1 < rows[row].len() => focus_at((row, col + 1)),
                        "Enter" if row < 2 => {
                            if let Some((_, action)) = rows[row].get(col) {
                                action();
                            }
                        }
                        _ => {}
                    }
                })
            }
        })
        .when(!is_tv() && cfg!(target_arch = "wasm32"), {
            let web_key = web_key.clone();
            move |v| {
                v.on_key_down(move |event: &KeyEvent| {
                    web_key(event.key.as_ref());
                })
            }
        })
        .on_press(show_controls)
        .child(match casting.clone() {
            Some(device) => cast_screen(&item, device).into_element(),
            #[cfg(target_arch = "wasm32")]
            None => view().test_id(crate::player::web::HOST).absolute().inset_0().into_element(),
            #[cfg(not(target_arch = "wasm32"))]
            None => native("remux.exo")
                .element_ref(&video_ref)
                .absolute()
                .inset_0()
                .prop("source", source.clone())
                .prop("paused", move || json!(paused.get()))
                .prop("subtitle", move || json!(subtitle.get().unwrap_or(-1)))
                .prop("audio", move || json!(current_audio.get().unwrap_or(-1)))
                .prop("rate", move || json!(rate.get()))
                .on_native_event({
                    let on_state = on_state.clone();
                    move |event: &NativeEvent| {
                        if &*event.name == "state" {
                            on_state(&event.data);
                        }
                    }
                })
                .into_element(),
        })
        // The subtitle the player read, over the bottom of the picture.
        .child(when(
            move || !cues.with(String::is_empty),
            move || {
                view().absolute().left_0().right_0().bottom(px(if controls.get() { 160.0 } else { 48.0 })).items_center().px_8().child(
                    text(move || cues.get())
                        .text_center()
                        .text_size(px(if is_tv() { 30.0 } else { 20.0 }))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme().foreground)
                        .px_2()
                        .rounded_md()
                        .bg(scrim()),
                )
            },
        ))
        .child(when(move || buffering.get() && failed.get().is_none(), || view().absolute().inset_0().items_center().justify_center().child(ui::spinner())))
        // The controls: the title and a way out above, playing and the time below.
        .child(when(
            move || controls.get(),
            move || {
                let title = title.clone();
                let toggle = toggle.clone();
                view()
                    .absolute()
                    .inset_0()
                    .justify_between()
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .pt_safe_area_plus(px(12.0))
                            .px_4()
                            .pb_8()
                            .bg_linear_to_b()
                            .gradient_from(scrim())
                            .gradient_to(theme().background.opacity(0.0))
                            .child(round_button(Phosphor::CaretLeft, "Close", stop).element_ref(&close_button))
                            .child(text(title).flex_1().text_lg().font_weight(FontWeight::SEMIBOLD).number_of_lines(1).text_color(theme().foreground))
                            // The web: the volume and full screen (a phone and a TV have keys and a
                            // screen of their own).
                            .child(cfg!(target_arch = "wasm32").then(|| volume_control(volume, muted)))
                            .child(has_audio_choice.then(|| round_button(Phosphor::SpeakerHigh, "Audio", move || audio_open.set(true)).element_ref(&audio_button)))
                            .child(has_subtitles.then(|| round_button(Phosphor::Subtitles, "Subtitles", move || subtitles_open.set(true)).element_ref(&subtitles_button)))
                            // Casting, from a phone, where there is a Chromecast to cast to.
                            .child(when(
                                crate::components::can_cast,
                                move || round_button(move || if crate::cast::active().is_some() { Phosphor::ScreencastFill.src() } else { Phosphor::Screencast.src() }, "Cast", crate::components::open_cast_sheet),
                            ))
                            .child(round_button(Phosphor::DotsThree, "Speed and version", move || options_open.set(true)).element_ref(&options_button))
                            .child(cfg!(target_arch = "wasm32").then(|| {
                                round_button(Phosphor::CornersOut, "Full screen", || {
                                    #[cfg(target_arch = "wasm32")]
                                    crate::player::web::WebVideo::toggle_fullscreen();
                                })
                            })),
                    )
                    .child(
                        view()
                            .flex_row()
                            .justify_center()
                            .items_center()
                            .gap_8()
                            // A channel goes to the one before and after it; anything else 10 seconds.
                            .child(if live {
                                round_button(Phosphor::SkipBackFill, "Previous channel", move || zap(-1))
                                    .element_ref(&back_button)
                                    .style_when(move || !can_zap(-1), |s| s.opacity(0.4))
                                    .into_element()
                            } else {
                                round_button(Phosphor::ArrowCounterClockwise, "Back 10 seconds", move || seek_by(-10.0)).element_ref(&back_button).into_element()
                            })
                            .child(
                                round_button(
                                    // The glyph follows the state: play while paused, pause while playing.
                                    move || if paused.get() { Phosphor::PlayFill.src() } else { Phosphor::PauseFill.src() },
                                    "Play or pause",
                                    toggle,
                                )
                                .element_ref(&play_button),
                            )
                            .child(if live {
                                round_button(Phosphor::SkipForwardFill, "Next channel", move || zap(1))
                                    .element_ref(&forward_button)
                                    .style_when(move || !can_zap(1), |s| s.opacity(0.4))
                                    .into_element()
                            } else {
                                round_button(Phosphor::ArrowClockwise, "Forward 10 seconds", move || seek_by(10.0)).element_ref(&forward_button).into_element()
                            }),
                    )
                    .child(
                        view()
                            .px_4()
                            .pt_8()
                            .pb_safe_area_plus(px(16.0))
                            .gap_2()
                            .bg_linear_to_t()
                            .gradient_from(scrim())
                            .gradient_to(theme().background.opacity(0.0))
                            // A channel has no time to go along: LIVE instead.
                            .child(if live {
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(view().size(px(10.0)).rounded_full().bg(crate::ui::tokens::live()))
                                    .child(text("LIVE").text_sm().font_weight(FontWeight::BOLD).text_color(theme().foreground))
                                    .into_element()
                            } else {
                                seek_bar(position, duration, seeking, seek_to, seek_stop, show_controls).into_element()
                            }),
                    )
            },
        ))
        // The corner: "Up next" near the end, else the skip of what plays now; also with the
        // controls hidden.
        .child(when(corner, move || {
            view()
                .absolute()
                .right(px(24.0))
                .bottom(px(if is_tv() { 160.0 } else { 140.0 }))
                .child(dynamic(move || match (show_next.get(), next.get().flatten(), skippable.get()) {
                    (true, Some(episode), _) => untracked(|| up_next(episode, corner_button, play_button).into_element()),
                    (_, _, Some((kind, end))) => ui::text_button(ui::Variant::Primary, ui::Size::Regular, Phosphor::SkipForward, kind.skip_label(), move || {
                        seek_to(end);
                        position.set(end / 1000.0);
                    })
                    .element_ref(&corner_button)
                    .into_element(),
                    _ => view().into_element(),
                }))
        }))
        .child(when(
            move || failed.get().is_some(),
            {
                let request = request.clone();
                move || failure(move || SharedString::from(format!("Could not play it: {}", failed.get().unwrap_or_default())), request.from(position.get_untracked() * 1000.0))
            },
        ))
        .child(ui::sheet(audio_open, "Audio", move || {
            let pick = pick_audio.clone();
            ui::menu_group(
                audio_tracks
                    .iter()
                    .cloned()
                    .map(|(index, label, codec)| {
                        let pick = pick.clone();
                        ui::menu_row((current_audio.get() == Some(index)).then_some(Phosphor::Check), label, false, move || pick(index, codec.clone()))
                    })
                    .collect(),
            )
        }))
        .child(ui::sheet(subtitles_open, "Subtitles", move || {
            let pick = pick_subtitle.clone();
            let off = pick.clone();
            let mut rows = vec![ui::menu_row(subtitle.get().is_none().then_some(Phosphor::Check), "Off", false, move || off(None, false))];
            for (index, label, burned) in subtitle_tracks_sheet.clone() {
                let pick = pick.clone();
                rows.push(ui::menu_row((subtitle.get() == Some(index)).then_some(Phosphor::Check), label, false, move || pick(Some(index), burned)));
            }
            ui::menu_group(rows)
        }))
        .child(options_sheet(options_open, rate, item.clone(), resolved.source_id, request.clone(), position))
}

/// The seek bar, with the time gone and the time left. On a TV a stop of its own (`me`), where the
/// player's left and right go 10 seconds.
fn seek_bar(
    position: Signal<f64>,
    duration: Signal<f64>,
    seeking: Signal<bool>,
    seek_to: impl Fn(f64) + Copy + 'static,
    me: ElementRef,
    show_controls: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    view()
        .gap_2()
        .child(
            view()
                .rounded_full()
                .px_2()
                // A TV's stop: the player's keys go 10 seconds on it.
                .when(is_tv(), move |v| {
                    v.element_ref(&me)
                        .focusable(true)
                        .role(AccessibilityRole::Adjustable)
                        .label("Position")
                        .focus_visible(|s| s.bg(theme().foreground.opacity(0.2)))
                })
                .child(
                    slider()
                        .value(move || SliderValue::Single(position.get()))
                        .min(0.0)
                        .max(move || duration.get().max(1.0))
                        .step(0.0)
                        .accessibility_label("Position")
                        .on_change(move |value: &SliderValue| {
                            seeking.set(true);
                            position.set(value.end());
                            show_controls();
                        })
                        .on_change_end(move |value: &SliderValue| {
                            seeking.set(false);
                            seek_to(value.end() * 1000.0);
                        })
                        .h(px(28.0))
                        .justify_center()
                        .track_style(|s| s.h(px(5.0)).rounded_full().bg(track()))
                        .indicator_style(|s| s.h_full())
                        .range_style(|s| s.rounded_full().bg(theme().foreground))
                        .thumb_style(|s| s.size(px(14.0)).rounded_full().bg(theme().foreground)),
                ),
        )
        .child(
            view()
                .flex_row()
                .justify_between()
                .child(text(move || clock(position.get())).text_sm().tabular_nums().text_color(theme().foreground))
                .child(text(move || format!("-{}", clock(duration.get() - position.get()))).text_sm().tabular_nums().text_color(theme().foreground)),
        )
}

/// "Up next": the episode after this one, to play now (Remux's `NextEpisodeCard`).
fn up_next(episode: Media, me: ElementRef, back: ElementRef) -> impl IntoElement {
    let still = server().still_url(&episode, 480);
    let label = match (episode.season_number, episode.index_number) {
        (Some(season), Some(number)) => format!("S{season} E{number} · {}", episode.title),
        _ => episode.title.clone(),
    };
    let _ = back;
    button()
        .element_ref(&me)
        .w(px(if is_tv() { 360.0 } else { 280.0 }))
        .rounded_xl()
        .overflow_hidden()
        .bg(crate::ui::tokens::sheet())
        .border_1()
        .border_color(theme().border)
        .accessibility_label(format!("Up next: {label}"))
        .focus_visible(|s| s.outline(px(3.0)).outline_offset(px(3.0)).outline_color(theme().ring))
        .on_press(move || play(episode.clone()))
        .child(view().w_full().aspect_ratio(16.0 / 9.0).bg(crate::ui::tokens::group()).child(still.map(|url| image(url).size_full().object_fit(ResizeMode::Cover))))
        .child(
            view()
                .p_3()
                .gap_1()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(labeled_icon(Phosphor::PlayFill, "").size(px(14.0)).text_color(theme().foreground))
                        .child(text("Up next").text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme().muted_foreground)),
                )
                .child(text(label).text_sm().font_weight(FontWeight::SEMIBOLD).number_of_lines(2).text_color(theme().foreground)),
        )
}

/// The web's volume: mute, and how loud.
fn volume_control(volume: Signal<f64>, muted: Signal<bool>) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(round_button(
            move || if muted.get() || volume.get() == 0.0 { Phosphor::SpeakerSlash.src() } else { Phosphor::SpeakerHigh.src() },
            "Mute",
            move || muted.update(|muted| *muted = !*muted),
        ))
        .child(
            slider()
                .value(move || SliderValue::Single(if muted.get() { 0.0 } else { volume.get() }))
                .min(0.0)
                .max(1.0)
                .step(0.0)
                .accessibility_label("Volume")
                .on_change(move |value: &SliderValue| {
                    volume.set(value.end());
                    if value.end() > 0.0 {
                        muted.set(false);
                    }
                })
                .w(px(120.0))
                .h(px(28.0))
                .justify_center()
                .track_style(|s| s.h(px(4.0)).rounded_full().bg(track()))
                .indicator_style(|s| s.h_full())
                .range_style(|s| s.rounded_full().bg(theme().foreground))
                .thumb_style(|s| s.size(px(12.0)).rounded_full().bg(theme().foreground)),
        )
}

/// Speed and version (Remux's options menu, besides the audio and the subtitles, which have
/// buttons of their own). Another version plays from where this one is.
fn options_sheet(open: Signal<bool>, rate: Signal<f64>, item: Media, playing: uuid::Uuid, request: Request, position: Signal<f64>) -> impl IntoElement {
    let item_id = item.id;
    let versions = resource(move || open.get().then_some(()), move |(), _| async move {
        server().media_sources(item_id).await.map_err(|error| error.to_string())
    });
    ui::sheet(open, "Speed and version", move || {
        let request = request.clone();
        view()
            .gap_3()
            .child(ui::menu_heading("Speed"))
            .child(ui::menu_group(
                SPEEDS
                    .iter()
                    .map(|speed| {
                        let speed = *speed;
                        let label = if speed == 1.0 { "Normal".to_owned() } else { format!("{speed}×") };
                        ui::menu_row(((rate.get() - speed).abs() < 0.01).then_some(Phosphor::Check), label, false, move || {
                            rate.set(speed);
                            open.set(false);
                        })
                    })
                    .collect(),
            ))
            .child(dynamic(move || {
                let list = versions.get().unwrap_or_default();
                if list.len() < 2 {
                    return view().into_element();
                }
                let request = request.clone();
                view()
                    .gap_3()
                    .child(ui::menu_heading("Version"))
                    .child(ui::menu_group(
                        list.into_iter()
                            .map(|source| {
                                let (title, detail) = (source.title(), source.subtitle().filter(|detail| *detail != source.title()));
                                let on = source.id == playing;
                                let request = request.clone();
                                ui::menu_choice_checked(title, detail, on, move || {
                                    open.set(false);
                                    if !on {
                                        let mut next = request.from(position.get_untracked() * 1000.0);
                                        next.source = Some(source.clone());
                                        next.audio = None;
                                        next.burned = None;
                                        start(next);
                                    }
                                })
                            })
                            .collect(),
                    ))
                    .into_element()
            }))
    })
}

/// A round button of the controls.
fn round_button(glyph: impl IntoReactive<nsh_core::element::ImageSrc>, label: &'static str, on_press: impl Fn() + 'static) -> nsh_base::ButtonElement {
    button()
        .size(px(56.0))
        .rounded_full()
        .items_center()
        .justify_center()
        .bg(scrim())
        .text_color(theme().foreground)
        .accessibility_label(label)
        .transition_transform()
        .duration(motion::FAST)
        .active(|s| s.scale(motion::ICON_PRESS_SCALE))
        .focus_visible(|s| s.bg(theme().primary).text_color(theme().primary_foreground))
        .on_press(on_press)
        .child(labeled_icon(glyph, label).size(px(28.0)))
}

/// What shows while a Chromecast plays: the title's picture, and where it plays.
fn cast_screen(item: &Media, device: crate::cast::Device) -> impl IntoElement {
    let backdrop = server().image_url_at(item, crate::media::ImageType::Backdrop, 1280);
    view()
        .absolute()
        .inset_0()
        .items_center()
        .justify_center()
        .gap_3()
        .child(backdrop.map(|url| image(url).absolute().inset_0().object_fit(ResizeMode::Cover).opacity(0.3)))
        .child(labeled_icon(Phosphor::ScreencastFill, "").size(px(48.0)).text_color(theme().foreground))
        .child(text(format!("Playing on {}", device.name)).text_lg().font_weight(FontWeight::SEMIBOLD).text_color(theme().foreground))
}
