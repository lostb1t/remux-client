//! What can be done with a title, in a sheet (Remux's `CardMenu` and `PlaylistSheet`): mark it
//! watched, favorite it, put it on the watchlist (a Remux server), add it to a playlist, go to its
//! show; and first, as in Remux, play it. Opened by a long press on a card, and by the hero's More.

use std::sync::Arc;

use nsh_core::prelude::*;
use nsh_icon_phosphor::Phosphor;

use crate::hooks::server;
use crate::media::{Media, MediaType};
use crate::nav::{self, Route};
use crate::server::JellyfinServer;
use crate::ui;

/// The title whose menu is open, if any.
#[derive(Clone, Default, PartialEq)]
struct Target(Option<Media>);

/// Opens the menu of `media`, over whatever shows.
pub fn open_item_menu(media: Media) {
    global::<Target>().set(Target(Some(media)));
}

/// The one menu of the app, for the title [`open_item_menu`] opened it for: in the shell.
pub fn item_menu_host() -> impl IntoElement {
    let target = global::<Target>();
    let open = signal(false);
    // Open while there is a title; closed (a swipe, back), the title goes.
    effect(move || open.set(target.with(|target| target.0.is_some())));
    effect(move || {
        if !open.get() {
            untracked(|| target.set(Target(None)));
        }
    });
    dynamic(move || match target.with(|target| target.0.clone()) {
        Some(media) => item_menu(media, open).into_element(),
        None => view().into_element(),
    })
}

/// The sheet of `media`, open while `open` is.
fn item_menu(media: Media, open: Signal<bool>) -> impl IntoElement {
    let picking = signal(false);
    let title = media.title.clone();
    // A remote starts on the first choice (Play), as in Remux.
    let first = element_ref();
    let playable = matches!(media.media_type, MediaType::Movie | MediaType::Episode);
    ui::sheet_focusing(open, title, playable.then_some(first), move || {
        let media = media.clone();
        dynamic(move || {
            let media = media.clone();
            if picking.get() { playlist_picker(media, open, picking).into_element() } else { choices(media, open, picking, first).into_element() }
        })
    })
}

/// Runs `change` on the server, with `value` shown at once and taken back when it fails.
fn toggle(
    value: Signal<bool>,
    media: &Media,
    change: impl Fn(Arc<JellyfinServer>, bool, Media) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>>>> + 'static,
) -> impl Fn() + Clone + 'static {
    let media = media.clone();
    let change = std::rc::Rc::new(change);
    move || {
        let next = !value.get_untracked();
        value.set(next);
        let future = change(server(), next, media.clone());
        spawn(async move {
            if let Err(error) = future.await {
                log::error!("could not change it: {error}");
                value.set(!next);
            }
        });
    }
}

fn choices(media: Media, open: Signal<bool>, picking: Signal<bool>, first: ElementRef) -> impl IntoElement {
    let watched = signal(media.is_watched());
    let favorite = signal(media.is_favorite());
    let watchlist = signal(media.is_watchlist());
    let remux = resource(|| Some(()), |(), _| async { Ok::<_, String>(server().is_remux().await) });

    let toggle_watched = toggle(watched, &media, |server, on, media| Box::pin(async move { server.set_watched(on, &media).await }));
    let toggle_favorite = toggle(favorite, &media, |server, on, media| Box::pin(async move { server.set_favorite(on, &media).await }));
    let toggle_watchlist = toggle(watchlist, &media, |server, on, media| Box::pin(async move { server.set_watchlist(on, &media).await }));

    let mut rows = Vec::new();
    // Play (or Resume) a film or an episode, through the choice of its version.
    if matches!(media.media_type, MediaType::Movie | MediaType::Episode) {
        let to_play = media.clone();
        let label = if media.progress().is_some() { "Resume" } else { "Play" };
        rows.push(ui::menu_row_at(Some(first), Some(Phosphor::Play), label, false, move || {
            open.set(false);
            crate::components::play_with_choice(to_play.clone());
        }));
    }
    rows.extend([
        ui::menu_row(
            Some(Phosphor::CheckCircle),
            move || SharedString::from(if watched.get() { "Mark as unwatched" } else { "Mark as watched" }),
            false,
            toggle_watched,
        ),
        ui::menu_row(
            Some(Phosphor::Heart),
            move || SharedString::from(if favorite.get() { "Remove from favorites" } else { "Add to favorites" }),
            false,
            toggle_favorite,
        ),
    ]);
    if media.media_type == MediaType::Audio && crate::music::available() {
        let (next, last) = (media.clone(), media.clone());
        rows.insert(0, ui::menu_row(Some(Phosphor::Queue), "Add to queue", false, move || {
            open.set(false);
            crate::music::add_to_queue(last.clone());
        }));
        rows.insert(0, ui::menu_row(Some(Phosphor::ArrowBendDownRight), "Play next", false, move || {
            open.set(false);
            crate::music::play_next(next.clone());
        }));
    }
    if matches!(media.media_type, MediaType::Movie | MediaType::Episode) {
        rows.push(ui::menu_row(Some(Phosphor::ListPlus), "Add to playlist", true, move || picking.set(true)));
    }
    if let (MediaType::Episode, Some(series)) = (&media.media_type, media.series_id) {
        let name = media.series_name.clone().unwrap_or_default();
        rows.push(ui::menu_row(Some(Phosphor::Television), "Go to show", true, move || {
            open.set(false);
            let _ = &name;
            nav::push(Route::MediaDetail { media_type: MediaType::Series, id: series });
        }));
    }
    view()
        .gap_4()
        .child(ui::menu_group(rows))
        // Only a Remux server has a watchlist.
        .child(when(
            move || remux.get().unwrap_or(false),
            move || {
                let toggle_watchlist = toggle_watchlist.clone();
                ui::menu_group(vec![ui::menu_row(
                    Some(Phosphor::BookmarkSimple),
                    move || SharedString::from(if watchlist.get() { "Remove from watchlist" } else { "Add to watchlist" }),
                    false,
                    toggle_watchlist,
                )])
            },
        ))
}

/// The user's playlists to add `media` to, and a new one (Remux's `PlaylistPicker`).
fn playlist_picker(media: Media, open: Signal<bool>, picking: Signal<bool>) -> impl IntoElement {
    let playlists = resource(|| Some(()), |(), _| async { server().playlists().await.map_err(|error| error.to_string()) });
    let name = signal(String::new());
    let error = signal(None::<String>);
    let add = {
        let media = media.clone();
        move |playlist: uuid::Uuid| {
            let media = media.clone();
            spawn(async move {
                match server().add_to_playlist(playlist, &media).await {
                    Ok(()) => open.set(false),
                    Err(failure) => error.set(Some(failure.to_string())),
                }
            });
        }
    };
    let create = {
        let media = media.clone();
        move |wanted: &str| {
            let wanted = wanted.trim().to_owned();
            if wanted.is_empty() {
                return;
            }
            let media = media.clone();
            spawn(async move {
                match server().create_playlist(&wanted, &media).await {
                    Ok(()) => open.set(false),
                    Err(failure) => error.set(Some(failure.to_string())),
                }
            });
        }
    };
    view()
        .gap_4()
        .child(ui::menu_group(vec![ui::menu_row(Some(Phosphor::CaretLeft), "Back", false, move || picking.set(false))]))
        .child(when(move || playlists.get().is_none(), || view().items_center().py_6().child(ui::spinner())))
        .child(dynamic(move || {
            let add = add.clone();
            let lists = playlists.get().unwrap_or_default();
            if lists.is_empty() {
                return view().into_element();
            }
            ui::menu_group(
                lists
                    .into_iter()
                    .map(|(id, title)| {
                        let add = add.clone();
                        ui::menu_row(Some(Phosphor::Playlist), title, false, move || add(id))
                    })
                    .collect(),
            )
            .into_element()
        }))
        .child(ui::field(name, "New playlist", create))
        .child(when(
            move || error.get().is_some(),
            move || ui::menu_note(error.get().unwrap_or_default()),
        ))
}
