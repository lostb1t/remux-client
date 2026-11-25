//! The video player, over the whole app (Remux's `components/player`): the stream the server
//! says, played by ExoPlayer (the native component `remux.exo`, `player/exo.rs`), Remux's controls over it, and what the server is told
//! (start, progress, stop), so Continue Watching and Next Up follow. On a TV, OK plays and pauses,
//! left and right go 10 seconds, up and down show the controls, Back closes.

use std::rc::Rc;
use std::time::Duration;

use nsh_base::{SliderValue, button, slider, theme};
use nsh_core::prelude::*;
use nsh_icon::labeled_icon;
use nsh_icon_phosphor::Phosphor;

use crate::capabilities::Capabilities;
use crate::hooks::server;
use crate::media::{Media, MediaType};
use crate::player::api::{self as player_api, Stage};
use crate::player::reporter::Reporter;
use crate::player::engine::StreamKind;
use crate::player::stream::{self, ResolvedStream, SegmentKind};
use serde_json::json;
use crate::ui::{self, tokens::{is_tv, motion, scrim, track}};

/// What plays, if anything.
#[derive(Clone, Default, PartialEq)]
struct Playing(Option<Request>);

/// What to play: the title, and when it is played again for another audio track, that track and
/// where it was.
#[derive(Clone, PartialEq)]
struct Request {
    media: Media,
    /// The version chosen; else the server's first.
    source: Option<crate::media::MediaSource>,
    audio: Option<i64>,
    start_ms: Option<f64>,
}

/// Plays `media` from where it was left.
pub fn play(media: Media) {
    // Music and video do not play together.
    crate::music::pause();
    global::<Playing>().set(Playing(Some(Request { media, source: None, audio: None, start_ms: None })));
}

/// Plays this version of `media` from where it was left.
pub fn play_source(media: Media, source: crate::media::MediaSource) {
    crate::music::pause();
    global::<Playing>().set(Playing(Some(Request { media, source: Some(source), audio: None, start_ms: None })));
}

/// Closes the player when it is open (music starts).
pub fn close() {
    if global::<Playing>().with_untracked(|playing| playing.0.is_some()) {
        global::<Playing>().set(Playing(None));
    }
}

fn stop() {
    global::<Playing>().set(Playing(None));
    // The remote goes back to where playing started: the page's play button.
    if is_tv() {
        crate::tv_focus::Scope::top().to_play();
    }
}

/// "1:02:03" or "2:03".
fn clock(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as i64;
    let (hours, minutes, rest) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 { format!("{hours}:{minutes:02}:{rest:02}") } else { format!("{minutes}:{rest:02}") }
}

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
    let (audio, start_ms, chosen) = (request.audio, request.start_ms, request.source.clone());
    // Where it plays: here, or on the Chromecast cast to, which has a session of its own on the
    // server (being made: wait for it). Picking another plays it there from where it was.
    // (A memo: the list of devices changes now and then, which is no reason to ask again.)
    let target = computed(move || {
        let device = crate::cast::active();
        crate::cast::playback_server().map(|_| device)
    });
    // The page's own request: its versions, which the stream is asked for, by what plays it.
    let stream = resource(move || target.get(), move |device: Option<crate::cast::Device>, _| {
        let chosen = chosen.clone();
        async move {
        let server = untracked(crate::cast::playback_server).unwrap_or_else(server);
        let item = server.media_details(item_id).await.map_err(|error| error.to_string())?.ok_or("not found")?;
        // The version chosen (a version's id is its file's, the same to every session); else the
        // server's first.
        let source = chosen.or_else(|| item.media_sources.first().cloned());
        log::info!("playing version {:?}", source.as_ref().map(|source| (source.title(), source.id)));
        let capabilities = match &device {
            Some(device) => Capabilities::chromecast(&device.model),
            None => Capabilities::detect(),
        };
        let resolved = stream::resolve(&server, &item, source.as_ref(), &capabilities, audio, None)
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((item, Rc::new(resolved), device, server))
        }
    });
    // Where it was, when the place it plays changes.
    let resume_at = stored(start_ms);
    view()
        .absolute()
        .inset_0()
        .bg(theme().background)
        .child(dynamic(move || match (stream.get(), stream.error()) {
            (Some((item, resolved, device, playing_server)), _) => untracked(move || playback(item, resolved, resume_at.with(|at| *at), device, playing_server, resume_at).into_element()),
            (_, Some(error)) => view()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_4()
                .p_6()
                .child(ui::empty(format!("Could not play it: {error}")))
                .child(ui::text_button(ui::Variant::Secondary, ui::Size::Regular, Phosphor::X, "Close", stop))
                .into_element(),
            _ => view().flex_1().items_center().justify_center().child(ui::spinner()).into_element(),
        }))
}

fn playback(
    item: Media,
    resolved: Rc<ResolvedStream>,
    start_ms: Option<f64>,
    device: Option<crate::cast::Device>,
    playing_server: std::sync::Arc<crate::server::JellyfinServer>,
    resume_at: Stored<Option<f64>>,
) -> impl IntoElement {
    let start_ms = start_ms.unwrap_or_else(|| item.resume_ms());
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
    // The subtitle shown: the one of the stream to start with, else none.
    let subtitle = signal(resolved.subtitle);
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
    let subtitle_tracks: Vec<(i64, String)> = resolved.subtitle_tracks.iter().filter(|track| !track.burned).map(|track| (track.index, track.label.clone())).collect();
    let subtitles_open = signal(false);
    let audio_open = signal(false);
    // The audio tracks, and the one that plays: the player picks one of the file it plays as it is;
    // for another, the server makes another stream, which plays on from where this one was.
    let audio_tracks: Vec<(i64, String, String)> = resolved.audio_tracks.iter().map(|track| (track.index, track.label.clone(), track.codec.clone())).collect();
    let current_audio = signal(resolved.audio);
    let direct = resolved.kind == StreamKind::Direct;
    let pick_audio = {
        let item = item.clone();
        move |index: i64, codec: String| {
            audio_open.set(false);
            if direct && Capabilities::detect().player_picks_audio(&codec) {
                current_audio.set(Some(index));
                return;
            }
            let at = position.get_untracked() * 1000.0;
            global::<Playing>().set(Playing(Some(Request { media: item.clone(), source: None, audio: Some(index), start_ms: Some(at) })));
        }
    };
    let subtitle_tracks_sheet = subtitle_tracks.clone();
    let audio_tracks_button = audio_tracks.clone();
    // Where it was last, outside the player's state: the report as the player goes is made when
    // that state is gone.
    let last_at = Rc::new(std::cell::Cell::new(start_ms));
    let reporter = Rc::new(
        Reporter::new(playing_server, resolved.item_id, resolved.source_id, resolved.play_session_id.clone(), resolved.method.clone())
            .with_parents([item.season_id, item.series_id]),
    );
    let report = {
        let reporter = reporter.clone();
        move |stage: Stage| {
            let reporter = reporter.clone();
            let (at, still) = (position.get_untracked() * 1000.0, paused.get_untracked());
            spawn(async move { reporter.report(stage, at, still, None, None).await });
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
    let show_controls = move || {
        controls.set(true);
        let this = touched.with(|touched| {
            *touched += 1;
            *touched
        });
        set_timeout(Duration::from_secs(4), move || {
            if touched.with(|touched| *touched) == this && !paused.get_untracked() {
                controls.set(false);
            }
        });
    };
    // To a place in the stream (ms), on what plays it.
    // The `<video>` of the web, once it is in the page (`player/web.rs`).
    #[cfg(target_arch = "wasm32")]
    let web_video = stored(None::<Rc<crate::player::web::WebVideo>>);
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
    // The segment the position is in, to skip it (an intro, a recap, the credits).
    let segments = resolved.segments.clone();
    let skippable = computed(move || {
        let at = position.get() * 1000.0;
        segments.iter().find(|segment| at >= segment.start_ms && at < segment.end_ms - 1000.0).map(|segment| (segment.kind, segment.end_ms))
    });
    let next_item = item.clone();
    let title = match (&item.media_type, &item.series_name) {
        (MediaType::Episode, Some(show)) => format!("{show} · {}", item.title),
        _ => item.title.clone(),
    };

    // How it plays, from ExoPlayer's snapshot or the Chromecast's.
    let on_state = Rc::new(move |state: &serde_json::Value| {
        let number = |key: &str| state.get(key).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let flag = |key: &str| state.get(key).and_then(serde_json::Value::as_bool).unwrap_or(false);
        cues.set(state.get("cues").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned());
        if let Some(error) = state.get("error").and_then(serde_json::Value::as_str).filter(|error| !error.is_empty()) {
            failed.set(Some(error.to_owned()));
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
            report(Stage::Start);
            show_controls();
        }
        // At the end: the next episode of a show, else the player closes.
        if flag("ended") && !ended.get_untracked() {
            ended.set(true);
            let item = next_item.clone();
            spawn(async move {
                match player_api::next_episode(&server(), &item).await {
                    Ok(Some(next)) => play(next),
                    _ => stop(),
                }
            });
        }
    });
    // The web: its `<video>` goes into the view drawn for it once the page has that view, and is
    // read four times a second as ExoPlayer's snapshot is.
    #[cfg(target_arch = "wasm32")]
    if cast_engine.with(|engine| engine.is_none()) {
        let (url, hls, tracks, shown) = (resolved.url.clone(), resolved.kind == StreamKind::Hls, resolved.subtitle_tracks.clone(), resolved.subtitle);
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
        effect(move || {
            let still = paused.get();
            untracked(|| web_video.with(|slot| slot.clone()).map(|video| video.set_paused(still)));
        });
        effect(move || {
            let shown = subtitle.get();
            untracked(|| web_video.with(|slot| slot.clone()).map(|video| video.set_subtitle(shown)));
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
        engine.load(resolved.url.clone(), resolved.kind == StreamKind::Hls, start_ms, subtitles, resolved.subtitle);
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
        let (pause_engine, subtitle_engine) = (engine.clone(), engine);
        effect(move || {
            let still = paused.get();
            untracked(|| pause_engine.set_paused(still));
        });
        effect(move || {
            let shown = subtitle.get();
            untracked(|| subtitle_engine.set_subtitle(shown));
        });
    }

    // The remote's keys come to the player, not to the page under it.
    let root = element_ref();
    if is_tv() {
        set_timeout(Duration::from_millis(100), move || root.focus());
    }
    view()
        .element_ref(&root)
        .flex_1()
        .bg(theme().background)
        // The remote: OK plays and pauses, left and right go 10 seconds, up and down show the
        // controls. With the controls showing, the keys are the buttons'.
        .when(is_tv(), {
            let toggle = toggle.clone();
            move |v| {
                v.capture_keys(crate::tv_focus::capture(&["left", "right", "up", "down", "enter"])).on_key_down(move |event: &nsh_core::keyboard::KeyEvent| {
                    if controls.get_untracked() && !matches!(event.key.as_ref(), "ArrowLeft" | "ArrowRight") {
                        show_controls();
                        return;
                    }
                    match event.key.as_ref() {
                        "ArrowLeft" => seek_by(-10.0),
                        "ArrowRight" => seek_by(10.0),
                        "Enter" => toggle(),
                        _ => show_controls(),
                    }
                })
            }
        })
        .focusable(is_tv())
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
                view().absolute().left_0().right_0().bottom(px(if controls.get() { 140.0 } else { 48.0 })).items_center().px_8().child(
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
        .child(when(
            move || failed.get().is_some(),
            move || view().absolute().inset_0().items_center().justify_center().p_6().child(ui::empty(move || SharedString::from(format!("Could not play it: {}", failed.get().unwrap_or_default())))),
        ))
        .child(when(move || buffering.get(), || view().absolute().inset_0().items_center().justify_center().child(ui::spinner())))
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
                            .child(round_button(Phosphor::CaretLeft, "Close", stop))
                            .child(text(title).flex_1().text_lg().font_weight(FontWeight::SEMIBOLD).number_of_lines(1).text_color(theme().foreground))
                            .child((audio_tracks_button.len() > 1).then(|| round_button(Phosphor::SpeakerHigh, "Audio", move || audio_open.set(true))))
                            .child((!subtitle_tracks.is_empty()).then(|| round_button(Phosphor::Subtitles, "Subtitles", move || subtitles_open.set(true))))
                            // Casting, from a phone, where there is a Chromecast to cast to.
                            .child(when(
                                crate::components::can_cast,
                                move || round_button(move || if crate::cast::active().is_some() { Phosphor::ScreencastFill.src() } else { Phosphor::Screencast.src() }, "Cast", crate::components::open_cast_sheet),
                            )),
                    )
                    .child(
                        view()
                            .flex_row()
                            .justify_center()
                            .items_center()
                            .gap_8()
                            .child(round_button(Phosphor::ArrowCounterClockwise, "Back 10 seconds", move || seek_by(-10.0)))
                            .child(round_button(
                                // The glyph follows the state: play while paused, pause while playing.
                                move || if paused.get() { Phosphor::PlayFill.src() } else { Phosphor::PauseFill.src() },
                                "Play or pause",
                                toggle,
                            ))
                            .child(round_button(Phosphor::ArrowClockwise, "Forward 10 seconds", move || seek_by(10.0))),
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
                            .child(when(
                                move || skippable.get().is_some(),
                                move || {
                                    let (kind, end) = skippable.get().unwrap_or((SegmentKind::Intro, 0.0));
                                    view().items_end().child(ui::text_button(ui::Variant::Primary, ui::Size::Regular, Phosphor::SkipForward, kind.skip_label(), move || {
                                        seek_to(end);
                                        position.set(end / 1000.0);
                                    }))
                                },
                            ))
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
                            )
                            .child(
                                view()
                                    .flex_row()
                                    .justify_between()
                                    .child(text(move || clock(position.get())).text_sm().tabular_nums().text_color(theme().foreground))
                                    .child(text(move || format!("-{}", clock(duration.get() - position.get()))).text_sm().tabular_nums().text_color(theme().foreground)),
                            ),
                    )
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
            let mut rows = vec![ui::menu_row(subtitle.get().is_none().then_some(Phosphor::Check), "Off", false, move || {
                subtitle.set(None);
                subtitles_open.set(false);
            })];
            for (index, label) in subtitle_tracks_sheet.clone() {
                rows.push(ui::menu_row((subtitle.get() == Some(index)).then_some(Phosphor::Check), label, false, move || {
                    subtitle.set(Some(index));
                    subtitles_open.set(false);
                }));
            }
            ui::menu_group(rows)
        }))
}

/// A round button of the controls.
fn round_button(glyph: impl IntoReactive<nsh_core::element::ImageSrc>, label: &'static str, on_press: impl Fn() + 'static) -> impl IntoElement {
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
