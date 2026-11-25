//! A titled row of cards that scrolls sideways (Remux's `GenericMediaList`, horizontal): the rows
//! of Home, and of a title's, a person's and a collection's page.

use nsh_base::theme;
use nsh_core::keyboard::KeyEvent;
use nsh_core::prelude::*;

use crate::hooks::server;
use crate::media::{CardVariant, Media};
use crate::server::{self as api, MediaQuery};
use crate::tv_focus;
use crate::ui::{self, tokens::is_tv};

/// The room above and below a row's cards, for a poster that grows with the remote's focus.
const FOCUS_ROOM: f32 = 16.0;

/// Two rows of placeholders while what rows there are is asked for.
pub fn row_skeletons() -> impl IntoElement {
    let (width, height) = ui::card_size(CardVariant::Poster);
    view().gap_6().children((0..2).map(move |_| {
        view()
            .gap_3()
            .child(ui::skeleton_box(px(160.0).into(), px(20.0).into()).ml_6())
            .child(
                view()
                    .flex_row()
                    .gap(px(ui::card_gap()))
                    .pl_6()
                    .overflow_hidden()
                    .children((0..8).map(move |_| ui::skeleton_box(px(width).into(), px(height).into()))),
            )
    }))
}

/// A titled row of the items of `query`, in cards of `variant`: gone once it turns out to have
/// nothing. `place` is its place among the rows of a page, for the remote (`None`: no stop of its
/// own).
pub fn media_row(title: String, query: MediaQuery, variant: CardVariant, place: Option<usize>, on_open: impl Fn(Media) + Copy + 'static) -> impl IntoElement {
    let (width, height) = ui::card_size(variant);
    let focus = tv_focus::Scope::current();
    // The pages loaded so far: `None` until the first is in.
    let items = signal(None::<Vec<Media>>);
    let more = stored(true);
    let loading = stored(false);
    let page = query.limit;
    let load = move || {
        if loading.with(|loading| *loading) || !more.with(|more| *more) {
            return;
        }
        loading.with(|loading| *loading = true);
        let offset = items.with_untracked(|items| items.as_ref().map_or(0, Vec::len)) as u32;
        let query = MediaQuery { offset, ..query.clone() };
        spawn(async move {
            let found = api::get_media_cached(server(), &query).await.unwrap_or_default();
            more.with(|more| *more = found.len() as u32 >= page && page > 0);
            loading.with(|loading| *loading = false);
            items.update(|items| items.get_or_insert_with(Vec::new).extend(found));
        });
    };
    load();
    let count = move || items.with(|items| items.as_ref().map_or(0, Vec::len));
    let loaded = move || items.with(|items| items.is_some());
    view()
        .gap_2()
        .style_when(move || loaded() && count() == 0, |s| s.hidden())
        // Up and down go to the first card of the row above or below (or Play above the first),
        // not to the card nearest to the one with the focus.
        .when(is_tv() && place.is_some(), move |v| {
            let place = place.unwrap_or_default();
            v.capture_keys(tv_focus::capture(&["up", "down"])).on_key_down(move |event: &KeyEvent| match event.key.as_ref() {
                "ArrowDown" => {
                    focus.to_row_after(Some(place));
                }
                "ArrowUp" => {
                    focus.to_row_before(place);
                }
                _ => {}
            })
        })
        .child(
            text(title)
                .px_6()
                .text_xl()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme().foreground),
        )
        .child(
            when(move || !loaded(), move || {
                view()
                    .flex_row()
                    .gap(px(ui::card_gap()))
                    .pl_6()
                    .py(px(FOCUS_ROOM))
                    .overflow_hidden()
                    .children((0..8).map(move |_| ui::skeleton_box(px(width).into(), px(height).into())))
            })
            .otherwise(move || {
                // A plain scroll view with the cards of the row (a page of them, as in Remux), not
                // a `virtual_list`: the remote goes to a row's first card, which a list that
                // reuses its cells for other items has no fixed view of.
                let load = load.clone();
                scroll()
                    .horizontal()
                    .scrollbar(false)
                    // More cards when the row comes to rest within two screens of its end (Remux
                    // asks for the next page while scrolling).
                    .on_scroll_end(move |at: &Point<Points>| {
                        let length = count() as f32 * (width + ui::card_gap());
                        if at.x.0 + 3.0 * window().width.0 >= length {
                            load();
                        }
                    })
                    .h(px(height + 2.0 * FOCUS_ROOM + if variant == CardVariant::Square { 44.0 } else { 0.0 }))
                    .child(
                        view()
                            .flex_row()
                            .gap(px(ui::card_gap()))
                            .px_6()
                            .py(px(FOCUS_ROOM))
                            .child(each(
                                move || items.get().unwrap_or_default(),
                                |media: &Media| media.id,
                                move |media, at| {
                                    // The row's first card is where the remote comes in.
                                    let me = element_ref();
                                    if let (true, Some(place), 0) = (is_tv(), place, at.get_untracked()) {
                                        focus.set_row(place, me);
                                    }
                                    // Music has its name under the picture (Remux's `caption`).
                                    if variant == CardVariant::Square {
                                        ui::captioned_card(me, move || Some(media.get()), variant, on_open)
                                    } else {
                                        ui::media_card(me, move || Some(media.get()), variant, on_open)
                                    }
                                },
                            )),
                    )
            }),
        )
}
