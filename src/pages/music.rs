//! The pages of music (Remux's `AlbumView`, `ArtistView`, `PlaylistView`): an album's tracks, an
//! artist's songs and albums, a playlist. A track plays the list from there, in the music player.

use nsh_base::{theme};
use nsh_core::prelude::*;
use std::rc::Rc;

use uuid::Uuid;

use crate::components::{media_grid, page_header, play_buttons};
use crate::hooks::server;
use crate::media::{ImageType, Media, MediaType, Sort};
use crate::nav::{self, Route};
use crate::server::{self as api, MediaQuery};
use crate::ui::{self, tokens::{group, is_wide}};

/// A page without the system's header.
fn page(content: impl IntoElement) -> AnyElement {
    // Every page of music opens over a place: Back with a swipe from the left edge.
    crate::stack::page(content)
}

/// The side padding of a page.
fn inset() -> f32 {
    if is_wide() { 24.0 } else { 16.0 }
}

/// "3:45".
fn duration(seconds: i64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// A track in a list: its number, its title, who plays it when that is another than the album's,
/// and how long it is. Pressed, the list plays from it.
fn track_row(number: Option<i64>, track: &Media, show_artist: bool, list: Rc<Vec<Media>>, at: usize) -> AnyElement {
    let artist = track.artist.clone().filter(|_| show_artist);
    // The one that plays shows so.
    let id = track.id;
    let playing = move || crate::music::queue().with(|queue| queue.current().is_some_and(|current| current.id == id));
    view()
        .on_press(move || crate::music::play((*list).clone(), at))
        .focusable(true)
        .role(AccessibilityRole::Button)
        .label(track.title.clone())
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .min_h(px(52.0))
        .active(|s| s.bg(theme().accent))
        .focus_visible(|s| s.bg(theme().accent))
        .on_long_press({
            let track = track.clone();
            move |_: &PressEvent| crate::components::open_item_menu(track.clone())
        })
        .child(number.map(|number| text(number.to_string()).w(px(24.0)).text_sm().text_color(theme().muted_foreground)))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .child(text(track.title.clone()).text_base().number_of_lines(1).text_color(move || if playing() { theme().primary } else { theme().foreground }))
                .child(artist.map(|artist| text(artist).text_sm().number_of_lines(1).text_color(theme().muted_foreground))),
        )
        .child(track.runtime_seconds.map(|seconds| text(duration(seconds)).text_sm().text_color(theme().muted_foreground)))
        .into_element()
}

/// The disc of a track, for an album of more than one.
fn disc(track: &Media) -> Option<i64> {
    track.season_number
}

/// An album: its cover, who made it, and its tracks in order.
pub fn album(id: Uuid, name: String) -> AnyElement {
    let details = resource(|| Some(()), move |(), _| async move { Ok::<_, String>(server().media_details(id).await.ok().flatten()) });
    let tracks = resource(|| Some(()), move |(), _| async move {
        let query = MediaQuery::builder().parent(id).types(vec![MediaType::Audio]).sort(Sort::TrackNumber).limit(500).build();
        api::get_media_cached(server(), &query).await.map_err(|error| error.to_string())
    });
    page(scroll().flex_1().child(view().px(px(inset())).pb(crate::ui::tokens::bottom_room(140.0)).gap_6().child(page_header(name.clone(), true)).child(dynamic(move || {
        let album = details.get().flatten();
        let list = tracks.get();
        let name = name.clone();
        untracked(move || {
            let server = server();
            let list = Rc::new(list.unwrap_or_default());
            let cover = album.as_ref().and_then(|album| server.image_url_at(album, ImageType::Poster, 600));
            let artist = album
                .as_ref()
                .and_then(|album| album.album_artist.clone().zip(album.album_artist_id))
                .or_else(|| list.first().and_then(|track| track.artist.clone().zip(track.artist_id)));
            let album_artist = artist.as_ref().map(|(name, _)| name.clone());
            use chrono::Datelike;
            let year = album.as_ref().and_then(|album| album.release_date).map(|date| date.year().to_string());
            let count = (!list.is_empty()).then(|| format!("{} {}", list.len(), if list.len() == 1 { "song" } else { "songs" }));
            let minutes: i64 = list.iter().filter_map(|track| track.runtime_seconds).sum::<i64>() / 60;
            let total = (minutes > 0).then(|| format!("{minutes} min"));
            let facts = [year, count, total].into_iter().flatten().collect::<Vec<_>>().join(" · ");
            let many_discs = list.iter().filter_map(disc).any(|disc| disc > 1);
            let size = if is_wide() { 208.0 } else { 144.0 };
            let mut groups: Vec<(Option<i64>, Vec<AnyElement>)> = Vec::new();
            for (at, track) in list.iter().enumerate() {
                let this_disc = if many_discs { disc(track) } else { None };
                if groups.last().is_none_or(|(at, _)| *at != this_disc) {
                    groups.push((this_disc, Vec::new()));
                }
                let show_artist = track.artist.is_some() && track.artist != album_artist;
                groups.last_mut().map(|(_, rows)| rows.push(track_row(track.index_number, track, show_artist, list.clone(), at)));
            }
            view()
                .gap_6()
                .child(
                    view()
                        .flex_row()
                        .gap_4()
                        .child(
                            view()
                                .size(px(size))
                                .rounded_xl()
                                .overflow_hidden()
                                .bg(group())
                                .child(cover.map(|url| image(url).size_full().object_fit(ResizeMode::Cover).label(name.clone()))),
                        )
                        .child(
                            view()
                                .flex_1()
                                .self_end()
                                .gap_2()
                                .child(artist.map(|(artist, artist_id)| {
                                    ui::menu_row(None, artist.clone(), true, move || nav::push(Route::Artist { id: artist_id, name: artist.clone() }))
                                }))
                                .child((!facts.is_empty()).then(|| text(facts).px_4().text_sm().text_color(theme().muted_foreground))),
                        ),
                )
                .child((!list.is_empty()).then(|| {
                    let list = list.clone();
                    play_buttons(move || (*list).clone())
                }))
                .child(view().gap_4().children(groups.into_iter().map(|(at, rows)| {
                    view()
                        .gap_2()
                        .child(at.map(|at| ui::menu_heading(format!("Disc {at}"))))
                        .child(ui::menu_group(rows))
                })))
                .child(tracks.error().map(|error| ui::menu_note(format!("Could not load the tracks: {error}"))))
                .into_element()
        })
    }))))
}

/// An artist: who they are, what they are known for, and their albums.
pub fn artist(id: Uuid, name: String) -> AnyElement {
    let details = resource(|| Some(()), move |(), _| async move { Ok::<_, String>(server().media_details(id).await.ok().flatten()) });
    let popular = resource(|| Some(()), move |(), _| async move {
        let query = MediaQuery::builder().artist(id).types(vec![MediaType::Audio]).sort(Sort::Plays).descending(true).limit(5).build();
        Ok::<_, String>(api::get_media_cached(server(), &query).await.unwrap_or_default())
    });
    let initial = name.chars().next().map(String::from).unwrap_or_default();
    let size = if is_wide() { 160.0 } else { 112.0 };
    let header = view()
        .gap_6()
        .pb_6()
        .child(page_header(name.clone(), true))
        .child(dynamic(move || {
            let portrait = details.get().flatten().and_then(|artist| server().portrait_url(&artist));
            let initial = initial.clone();
            view()
                .size(px(size))
                .rounded_full()
                .overflow_hidden()
                .bg(group())
                .items_center()
                .justify_center()
                .child(match portrait {
                    Some(url) => image(url).size_full().object_fit(ResizeMode::Cover).into_element(),
                    None => text(initial).text_4xl().font_weight(FontWeight::SEMIBOLD).text_color(theme().muted_foreground).into_element(),
                })
                .into_element()
        }))
        .child(dynamic(move || {
            let tracks = popular.get().unwrap_or_default();
            if tracks.is_empty() {
                return view().into_element();
            }
            let tracks = Rc::new(tracks);
            view()
                .gap_2()
                .child(play_buttons({
                    let tracks = tracks.clone();
                    move || (*tracks).clone()
                }))
                .child(text("Popular").text_xl().font_weight(FontWeight::SEMIBOLD).text_color(theme().foreground))
                .child(ui::menu_group(tracks.iter().enumerate().map(|(at, track)| track_row(Some(at as i64 + 1), track, false, tracks.clone(), at)).collect()))
                .into_element()
        }))
        .child(text("Albums").text_xl().font_weight(FontWeight::SEMIBOLD).text_color(theme().foreground));
    let query = MediaQuery::builder().album_artist(id).types(vec![MediaType::Album]).sort(Sort::Released).descending(true).build();
    page(media_grid(header, query, true))
}

/// A playlist: its tracks as a list, or its titles as a grid.
pub fn playlist(id: Uuid, name: String) -> AnyElement {
    let items = resource(|| Some(()), move |(), _| async move {
        let query = MediaQuery::builder().playlist(id).limit(500).build();
        server().media(&query).await.map_err(|error| error.to_string())
    });
    page(dynamic(move || {
        let name = name.clone();
        match (items.get(), items.error()) {
            (_, Some(error)) => view().px(px(inset())).child(page_header(name, true)).child(ui::empty(format!("Could not load the playlist: {error}"))).into_element(),
            (None, _) => view().px(px(inset())).child(page_header(name, true)).child(view().items_center().py_6().child(ui::spinner())).into_element(),
            (Some(list), _) if list.is_empty() => {
                view().px(px(inset())).child(page_header(name, true)).child(ui::empty("Nothing in this playlist yet.")).into_element()
            }
            (Some(list), _) if list.iter().all(|item| item.media_type == MediaType::Audio) => {
                let list = Rc::new(list);
                scroll()
                    .flex_1()
                    .child(
                        view()
                            .px(px(inset()))
                            .pb(crate::ui::tokens::bottom_room(140.0))
                            .gap_4()
                            .child(page_header(name, true))
                            .child({
                                let list = list.clone();
                                play_buttons(move || (*list).clone())
                            })
                            .child(ui::menu_group(list.iter().enumerate().map(|(at, track)| track_row(None, track, true, list.clone(), at)).collect())),
                    )
                    .into_element()
            }
            (Some(_), _) => media_grid(page_header(name, true), MediaQuery::builder().playlist(id).limit(500).build(), false).into_element(),
        }
    }))
}
