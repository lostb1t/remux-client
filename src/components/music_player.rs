//! What the screen shows of the music (Remux's `components/music`): a bar above the tabs with
//! what plays, and the page of what plays with its queue. The player itself is the native
//! component `remux.music` in here, an empty view, so the commands have somewhere to go.

use nsh_base::{SliderValue, button, slider, theme};
use nsh_core::prelude::*;
use nsh_icon::labeled_icon;
use nsh_icon_phosphor::Phosphor;

use crate::media::Media;
use crate::music::{self, Repeat};
use crate::nav::{self, Route};
use crate::ui::{self, tokens::{group, is_tv, motion, track}};

/// "3:45".
fn clock(ms: f64) -> String {
    let seconds = (ms.max(0.0) / 1000.0) as i64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// The music, once for the app: in the shell, over the pages.
pub fn music_host() -> impl IntoElement {
    let current = computed(|| music::queue().with(|queue| queue.current()));
    let current_id = computed(move || current.get().map(|track| track.id));
    // The server hears of each track as it starts and ends.
    effect(move || {
        let id = current_id.get();
        untracked(move || music::on_track(id));
    });
    // Back closes the queue first, then the page.
    on_back_press(|| {
        let open = music::expanded().get_untracked();
        if open.queue {
            music::expanded().update(|expanded| expanded.queue = false);
            true
        } else if open.open {
            music::expanded().set(Default::default());
            true
        } else {
            false
        }
    });
    let host = element_ref();
    music::attach(host);
    view()
        .absolute()
        .inset_0()
        .pointer_events(PointerEvents::BoxNone)
        .child(native_player(host))
        .child(when(move || current.get().is_some() && !music::expanded().get().open, mini_bar))
        .child(when(move || current.get().is_some() && music::expanded().get().open, now_playing))
}

#[cfg(target_os = "android")]
fn native_player(host: ElementRef) -> AnyElement {
    native(music::android::NAME)
        .element_ref(&host)
        .absolute()
        .size(px(1.0))
        .on_native_event(|event: &NativeEvent| {
            if &*event.name == "state" {
                music::on_state(&event.data);
            }
        })
        .into_element()
}

#[cfg(not(target_os = "android"))]
fn native_player(_host: ElementRef) -> AnyElement {
    view().into_element()
}

/// A round button with an icon, for the controls.
fn control(glyph: impl IntoReactive<nsh_core::element::ImageSrc>, label: &'static str, size: f32, on_press: impl Fn() + 'static) -> nsh_base::ButtonElement {
    button()
        .size(px(size + 20.0))
        .rounded_full()
        .items_center()
        .justify_center()
        .text_color(theme().foreground)
        .accessibility_label(label)
        .transition_transform()
        .duration(motion::FAST)
        .active(|s| s.scale(motion::ICON_PRESS_SCALE))
        .focus_visible(|s| s.bg(theme().primary).text_color(theme().primary_foreground))
        .on_press(on_press)
        .child(labeled_icon(glyph, label).size(px(size)))
}

/// What plays, above the tabs: the cover, the name, play and next. Pressed, it opens the page.
fn mini_bar() -> impl IntoElement {
    let current = computed(|| music::queue().with(|queue| queue.current()));
    let playing = computed(|| music::clock().with(|clock| clock.playing));
    let progress = computed(|| music::clock().with(|clock| if clock.duration_ms > 0.0 { (clock.position_ms / clock.duration_ms).clamp(0.0, 1.0) as f32 } else { 0.0 }));
    let bar = ui::glass_surface()
        .w_full()
        .max_w(px(448.0))
        .rounded_2xl()
        .overflow_hidden()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .p_2()
                .child(
                    button()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .rounded_xl()
                        .accessibility_label("Open the player")
                        .focus_visible(|s| s.bg(theme().accent))
                        .on_press(|| music::expanded().set(music::Expanded { open: true, queue: false }))
                        .child(cover_image(current, 48.0, 8.0))
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(text(move || current.get().map(|track| track.title).unwrap_or_default()).text_sm().font_weight(FontWeight::SEMIBOLD).number_of_lines(1).text_color(theme().foreground))
                                .child(text(move || current.get().and_then(|track| track.artist).unwrap_or_default()).text_xs().number_of_lines(1).text_color(theme().muted_foreground)),
                        ),
                )
                .child(control(move || if playing.get() { Phosphor::PauseFill.src() } else { Phosphor::PlayFill.src() }, "Play or pause", 24.0, music::toggle_play))
                .child(control(Phosphor::SkipForwardFill, "Next", 22.0, music::next))
                .child(control(Phosphor::X, "Close the player", 18.0, music::stop)),
        )
        .child(
            view()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(px(2.0))
                .bg(track())
                .child(view().h_full().bg(theme().foreground).w(move || Length::from(nsh_core::geometry::Percentage(progress.get())))),
        );
    let place = view().absolute().left_0().right_0().bottom_0().items_center().px_3().pointer_events(PointerEvents::BoxNone);
    if is_tv() {
        // A TV has no tabs: the bar sits in the bottom right corner.
        place.items_end().px_6().pb_6().child(bar.w(px(420.0)))
    } else {
        // Above the tab bar (52 high with 6 around it, 8 under it).
        place.pb_safe_area_plus(px(80.0)).child(bar)
    }
}

/// The cover of a track, or a note where it has none.
fn cover_image(track: Memo<Option<Media>>, size: f32, radius: f32) -> impl IntoElement {
    view()
        .size(px(size))
        .rounded(px(radius))
        .overflow_hidden()
        .bg(group())
        .items_center()
        .justify_center()
        .child(dynamic(move || match track.get().as_ref().and_then(music::cover) {
            Some(url) => image(url).size_full().object_fit(ResizeMode::Cover).into_element(),
            None => labeled_icon(Phosphor::MusicNotes, "").size(px(size * 0.4)).text_color(theme().muted_foreground).into_element(),
        }))
}

/// The page of what plays: a big cover, the time, the buttons, and the queue.
fn now_playing() -> impl IntoElement {
    let current = computed(|| music::queue().with(|queue| queue.current()));
    let playing = computed(|| music::clock().with(|clock| clock.playing));
    let buffering = computed(|| music::clock().with(|clock| clock.buffering && clock.playing));
    let duration = computed(|| music::clock().with(|clock| clock.duration_ms));
    let position = computed(|| music::clock().with(|clock| clock.position_ms));
    let error = computed(|| music::clock().with(|clock| clock.error.clone()));
    let shuffle = computed(|| music::queue().with(|queue| queue.shuffle));
    let repeat = computed(|| music::queue().with(|queue| queue.repeat));
    let queue_open = computed(|| music::expanded().get().queue);
    // Where the finger has the slider, while it does.
    let scrub = signal(None::<f64>);
    let shown = move || scrub.get().unwrap_or_else(|| position.get());
    let close = || music::expanded().set(Default::default());
    let size = if is_tv() { 360.0 } else { 320.0 };
    view()
        .absolute()
        .inset_0()
        .bg(theme().background)
        .overflow_hidden()
        // The cover, large and blurred, behind it all.
        .child(dynamic(move || {
            current
                .get()
                .as_ref()
                .and_then(music::cover)
                .map(|url| image(url).absolute().inset_0().object_fit(ResizeMode::Cover).opacity(0.35).blur(px(48.0)).scale(1.25).into_element())
                .unwrap_or_else(|| view().into_element())
        }))
        .child(
            view()
                .flex_1()
                .w_full()
                .max_w(px(576.0))
                .self_center()
                .gap_4()
                .px_6()
                .pt_safe_area_plus(px(16.0))
                .pb_safe_area_plus(px(24.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(control(Phosphor::CaretDown, "Close the player", 26.0, close))
                        .child(text(move || if queue_open.get() { "Queue" } else { "Now playing" }).text_sm().font_weight(FontWeight::MEDIUM).text_color(theme().muted_foreground))
                        .child(control(Phosphor::Queue, "Queue", 24.0, || music::expanded().update(|expanded| expanded.queue = !expanded.queue))),
                )
                .child(
                    when(move || queue_open.get(), queue_panel).otherwise(move || {
                        view()
                            .flex_1()
                            .justify_center()
                            .gap_6()
                            .child(view().self_center().child(cover_image(current, size, 16.0)))
                            .child(
                                view()
                                    .gap_1()
                                    .child(text(move || current.get().map(|track| track.title).unwrap_or_default()).text_2xl().font_weight(FontWeight::BOLD).number_of_lines(1).text_color(theme().foreground))
                                    .child(dynamic(move || {
                                        let track = current.get();
                                        let artist = track.as_ref().and_then(|track| track.artist.clone().zip(track.artist_id));
                                        let album = track.as_ref().and_then(|track| track.album.clone().zip(track.album_id));
                                        view()
                                            .gap_1()
                                            .child(artist.map(|(name, id)| link_text(name.clone(), false, move || {
                                                close();
                                                nav::push(Route::Artist { id, name: name.clone() });
                                            })))
                                            .child(album.map(|(name, id)| link_text(name.clone(), true, move || {
                                                close();
                                                nav::push(Route::Album { id, name: name.clone() });
                                            })))
                                            .into_element()
                                    }))
                                    .child(dynamic(move || {
                                        error.get().map(|error| text(error).text_sm().pt_1().text_color(theme().destructive).into_element()).unwrap_or_else(|| view().into_element())
                                    })),
                            )
                    })
                )
                .child(
                    view()
                        .gap_1()
                        .child(
                            slider()
                                .value(move || SliderValue::Single(shown()))
                                .min(0.0)
                                .max(move || duration.get().max(1.0))
                                .step(0.0)
                                .accessibility_label("Seek")
                                .on_change(move |value: &SliderValue| scrub.set(Some(value.end())))
                                .on_change_end(move |value: &SliderValue| {
                                    music::seek(value.end());
                                    scrub.set(None);
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
                                .child(text(move || clock(shown())).text_xs().tabular_nums().text_color(theme().muted_foreground))
                                .child(text(move || format!("-{}", clock(duration.get() - shown()))).text_xs().tabular_nums().text_color(theme().muted_foreground)),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(control(Phosphor::Shuffle, "Shuffle", 24.0, music::toggle_shuffle).style_when(move || !shuffle.get(), |s| s.opacity(0.5)))
                        .child(control(Phosphor::SkipBackFill, "Previous", 30.0, music::previous))
                        .child(
                            button()
                                .size(px(80.0))
                                .rounded_full()
                                .items_center()
                                .justify_center()
                                .bg(theme().foreground)
                                .text_color(theme().background)
                                .accessibility_label("Play or pause")
                                .transition_transform()
                                .duration(motion::FAST)
                                .active(|s| s.scale(motion::PRESS_SCALE))
                                .focus_visible(|s| s.bg(theme().primary).text_color(theme().primary_foreground))
                                .on_press(music::toggle_play)
                                .child(
                                    when(move || buffering.get(), || ui::spinner())
                                        .otherwise(move || labeled_icon(move || if playing.get() { Phosphor::PauseFill.src() } else { Phosphor::PlayFill.src() }, "").size(px(38.0))),
                                ),
                        )
                        .child(control(Phosphor::SkipForwardFill, "Next", 30.0, music::next))
                        .child(
                            control(move || if repeat.get() == Repeat::One { Phosphor::RepeatOnce.src() } else { Phosphor::Repeat.src() }, "Repeat", 24.0, music::cycle_repeat)
                                .style_when(move || repeat.get() == Repeat::Off, |s| s.opacity(0.5)),
                        ),
                ),
        )
}

/// The name of an artist or an album, which opens its page.
fn link_text(name: String, small: bool, on_press: impl Fn() + 'static) -> impl IntoElement {
    button()
        .self_start()
        .rounded_md()
        .accessibility_label(name.clone())
        .focus_visible(|s| s.bg(theme().accent))
        .on_press(on_press)
        .child(
            text(name)
                .when(small, |t| t.text_sm())
                .when(!small, |t| t.text_base())
                .number_of_lines(1)
                .text_color(theme().muted_foreground),
        )
}

/// What is queued, in the order it plays. Pressed, a track plays; the cross takes it out.
fn queue_panel() -> impl IntoElement {
    let tracks = computed(|| music::queue().with(|queue| queue.tracks.clone()));
    let index = computed(|| music::queue().with(|queue| queue.index.unwrap_or(0)));
    view()
        .flex_1()
        .gap_2()
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .px_1()
                .child(text(move || format!("{} tracks", tracks.get().len())).text_sm().text_color(theme().muted_foreground))
                .child(
                    button()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .accessibility_label("Clear the queue")
                        .focus_visible(|s| s.bg(theme().accent))
                        .on_press(music::stop)
                        .child(text("Clear").text_sm().font_weight(FontWeight::MEDIUM).text_color(theme().muted_foreground)),
                ),
        )
        .child(
            // A queue is an album or a playlist (500 at most): a plain scroll view.
            scroll().flex_1().child(view().gap_1().child(each(
                move || tracks.get().into_iter().enumerate().collect::<Vec<_>>(),
                |(place, track): &(usize, Media)| format!("{place}-{}", track.id),
                move |entry, _| {
                    let (place, track) = entry.get_untracked();
                    let (title, artist) = (track.title.clone(), track.artist.clone());
                    let current = move || index.get() == place;
                    view()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .rounded_lg()
                        .style_when(current, |s| s.bg(theme().accent))
                        .child(
                            button()
                                .flex_1()
                                .min_w(px(0.0))
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .px_2()
                                .py_2()
                                .rounded_lg()
                                .accessibility_label(title.clone())
                                .focus_visible(|s| s.bg(theme().accent))
                                .on_press(move || music::skip_to(place))
                                .child(
                                    view().w(px(24.0)).items_center().child(
                                        when(current, || labeled_icon(Phosphor::PlayFill, "Playing").size(px(14.0)).text_color(theme().foreground))
                                            .otherwise(move || text((place + 1).to_string()).text_xs().tabular_nums().text_color(theme().muted_foreground)),
                                    ),
                                )
                                .child(
                                    view()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .child(text(title).text_sm().font_weight(FontWeight::MEDIUM).number_of_lines(1).text_color(theme().foreground))
                                        .child(artist.map(|artist| text(artist).text_xs().number_of_lines(1).text_color(theme().muted_foreground))),
                                ),
                        )
                        .child(when(move || !current(), move || control(Phosphor::X, "Remove from the queue", 16.0, move || music::remove(place))))
                },
            ))),
        )
}

/// Play and Shuffle, for a list of tracks: an album, an artist, a playlist.
pub fn play_buttons(tracks: impl Fn() -> Vec<Media> + Clone + 'static) -> impl IntoElement {
    let shuffle_tracks = tracks.clone();
    view()
        .flex_row()
        .gap_3()
        .child(ui::text_button(ui::Variant::Primary, ui::Size::Regular, Phosphor::PlayFill, "Play", move || music::play(tracks(), 0)).min_w(px(0.0)))
        .child(ui::text_button(ui::Variant::Secondary, ui::Size::Regular, Phosphor::Shuffle, "Shuffle", move || music::shuffle_play(shuffle_tracks())).min_w(px(0.0)))
}
