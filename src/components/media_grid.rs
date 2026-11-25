//! A grid of the items of a query, loaded page by page as it is scrolled (Remux's
//! `GenericMediaList`, vertical): a library, a collection, the favorites, what a person is in. A
//! native list with columns, so only the cards near the screen exist.

use nsh_core::prelude::*;

use crate::hooks::server;
use crate::media::{CardVariant, Media, MediaType};
use crate::nav;
use crate::server::{self as api, MediaQuery};
use crate::ui::{self, tokens::{is_tv, is_wide}};

/// How many items a page of the grid asks for: it is scrolled through quickly (Remux: 60).
const PAGE: u32 = 60;

/// The pages loaded so far.
#[derive(Clone, PartialEq, Default)]
struct Pages {
    items: Vec<Media>,
    loading: bool,
    more: bool,
    error: Option<String>,
}

/// How many columns: about a poster of 110 points on a phone, 160 on a wide screen, six on a TV.
fn columns() -> u32 {
    let width = window().width.0;
    if is_tv() {
        6
    } else if is_wide() {
        ((width - 48.0) / 170.0).floor().max(4.0) as u32
    } else {
        ((width - 32.0) / 115.0).floor().max(3.0) as u32
    }
}

/// The grid of `query`'s items, under `header`; `caption` puts the name and year under each card.
pub fn media_grid(header: impl IntoElement, query: MediaQuery, caption: bool) -> impl IntoElement {
    // Albums and artists have square pictures.
    let square = !query.types.is_empty() && query.types.iter().all(|kind| matches!(kind, MediaType::Album | MediaType::MusicArtist));
    // Channels have their logos, wide.
    let wide = !query.types.is_empty() && query.types.iter().all(|kind| *kind == MediaType::Channel);
    let variant = if square {
        CardVariant::Square
    } else if wide {
        CardVariant::Landscape
    } else {
        CardVariant::Poster
    };
    let pages = signal(Pages { more: true, ..Pages::default() });
    let load = move || {
        let offset = pages.with_untracked(|pages| {
            (!pages.loading && pages.more && pages.error.is_none()).then_some(pages.items.len() as u32)
        });
        let Some(offset) = offset else { return };
        pages.update(|pages| pages.loading = true);
        let query = MediaQuery { offset, limit: PAGE, ..query.clone() };
        spawn(async move {
            let found = api::get_media_cached(server(), &query).await;
            pages.update(|pages| {
                pages.loading = false;
                match found {
                    Ok(found) => {
                        pages.more = found.len() as u32 >= PAGE;
                        pages.items.extend(found);
                    }
                    Err(error) => pages.error = Some(error.to_string()),
                }
            });
        });
    };
    load();
    // Pulled down to refresh: from the first page again (not while a page is on its way, which
    // would land among the new ones).
    let reload = {
        let load = load.clone();
        move || {
            if pages.with_untracked(|pages| !pages.loading) {
                pages.set(Pages { more: true, ..Pages::default() });
                load();
            }
        }
    };
    let open = nav::open_media;
    let count = move || pages.with(|pages| pages.items.len());
    let gap = ui::card_gap();
    crate::refresh::refreshable_with(virtual_list(count, move |at| {
        let media = move || pages.with(|pages| pages.items.get(at.index()).cloned());
        ui::grid_card(media, variant, caption, open)
    }), reload)
    .columns(columns())
    .column_gap(px(gap))
    .row_gap(px(if caption { 16.0 } else { gap }))
    .content_padding_x(px(if is_wide() { 24.0 } else { 16.0 }))
    .content_padding_bottom(crate::ui::tokens::bottom_room(140.0))
    .estimated_item_size(px(220.0))
    .header(header)
    .footer(
        view()
            .py_6()
            .items_center()
            .child(when(move || pages.with(|pages| pages.loading), ui::spinner))
            .child(when(
                move || pages.with(|pages| !pages.loading && pages.items.is_empty() && pages.error.is_none()),
                || ui::empty("Nothing here yet."),
            ))
            .child(when(
                move || pages.with(|pages| pages.error.is_some()),
                move || ui::empty(move || SharedString::from(pages.with(|pages| pages.error.clone()).unwrap_or_default())),
            )),
    )
    .on_load_more(load)
    .flex_1()
}
