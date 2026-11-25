//! The hero at the top of the home screen: what is being watched, then favorites and what is
//! popular, one at a time, swiped through (Remux: `HeroList`, `HeroItem`). NSH's carousel owns the
//! swiping, the looping and the dots; Remux fades one into the next, this slides.

use nsh_base::{carousel, carousel_content, carousel_item, carousel_pagination, carousel_pagination_item, theme};
use nsh_core::geometry::relative;
use nsh_core::keyboard::KeyEvent;
use nsh_core::prelude::*;

use crate::components::{hero_item, HeroKind};
use crate::hooks::server;
use crate::tv_focus;
use crate::media::{CatalogId, Media};
use crate::server::{self as api, MediaQuery};
use crate::ui::{self, tokens::{is_tv, is_wide, motion}};

/// How many items the hero goes through.
const HERO_COUNT: usize = 10;
/// The room of the dots under the hero, on a phone.
const DOTS_HEIGHT: f32 = 30.0;

/// The height of the hero: a phone shows the first row just below it, a wide screen leaves room
/// for it, a TV shows the hero almost the whole screen (Remux's `phone_height` and `size`).
fn hero_height() -> f32 {
    let screen = window();
    if is_tv() {
        screen.height.0 - 96.0
    } else if is_wide() {
        (screen.width.0 * 0.56).min(screen.height.0 * 0.72)
    } else {
        screen.height.0 * 0.7
    }
}

/// The height of a slide: the hero's, less the dots under it on a phone.
fn slide_height() -> f32 {
    hero_height() - if is_wide() { 0.0 } else { DOTS_HEIGHT }
}

/// The hero, `on_open` called with the item whose page is wanted.
pub fn hero(types: Vec<crate::media::MediaType>, on_open: impl Fn(Media) + Copy + 'static) -> impl IntoElement {
    // What the hero shows follows the same kinds as the rows.
    let query = MediaQuery::builder()
        .limit(HERO_COUNT as u32)
        .types(types)
        .catalog(CatalogId::Hero)
        .build();
    let items = resource(|| Some(()), move |(), _| {
        let query = query.clone();
        async move { api::get_media_cached(server(), &query).await.map_err(|error| error.to_string()) }
    });
    let list = move || items.get().unwrap_or_default();
    let index = signal(0u32);
    view()
        .w_full()
        .h(px(hero_height()))
        .bg(theme().background)
        .child(when(move || items.get().is_none(), || ui::skeleton_box(relative(1.0).into(), relative(1.0).into()).rounded_none()))
        .child(when(move || !list().is_empty(), move || {
            if is_tv() { tv_hero(list, index, on_open).into_element() } else { swiped_hero(list, index, on_open).into_element() }
        }))
}

/// A phone's hero: the items side by side in NSH's carousel, swiped with a finger.
fn swiped_hero(list: impl Fn() -> Vec<Media> + Copy + 'static, index: Signal<u32>, on_open: impl Fn(Media) + Copy + 'static) -> impl IntoElement {
    // The items are all there at once, and stay: the slides are made from them once, as the
    // carousel's direct children (it counts its slides among them).
    let items = untracked(list);
    let count = items.len();
    // Not `looping`: with it NSH 0.7.0 shows none of the slides (on Android), only the dots.
    // Remux goes round; this stops at the ends.
    carousel()
        .index(move || Some(index.get()))
        .on_change(move |at: &u32| index.set(*at))
        .accessibility_label("Featured")
        .flex_1()
        // A height of its own: in the column of the carousel a growing content stays empty.
        .child(
            carousel_content()
                .w_full()
                .h(px(slide_height()))
                .children(items.into_iter().enumerate().map(move |(at, media)| {
                    // Sizes in points: a slide sized by percentages gets none in the carousel's
                    // scroll view.
                    carousel_item()
                        .w(px(window().width.0))
                        .h(px(slide_height()))
                        .accessibility_label(format!("{} of {count}: {}", at + 1, media.title))
                        .child(hero_item(media, HeroKind::Home, at == 0, on_open))
                })),
        )
        .child(dots(list))
}

/// A TV's hero: one item at a time (a carousel is a stop of the remote of its own, where left and
/// right would change the item instead of moving along its buttons). The dots are the remote's
/// way through the items, as in Remux: the item of the dot with the focus shows.
fn tv_hero(list: impl Fn() -> Vec<Media> + Copy + 'static, index: Signal<u32>, on_open: impl Fn(Media) + Copy + 'static) -> impl IntoElement {
    let focus = tv_focus::Scope::current();
    view()
        .flex_1()
        .child(dynamic(move || {
            let at = index.get() as usize;
            untracked(move || match list().get(at).cloned() {
                Some(media) => hero_item(media, HeroKind::Home, at == 0, on_open).into_element(),
                None => view().into_element(),
            })
        }))
        .child(
            view()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(12.0))
                .flex_row()
                .justify_center()
                .gap(px(4.0))
                // Down goes to the first row, up back to Play.
                .capture_keys(tv_focus::capture(&["up", "down"]))
                .on_key_down(move |event: &KeyEvent| match event.key.as_ref() {
                    "ArrowDown" => {
                        focus.to_row_after(None);
                    }
                    "ArrowUp" => {
                        focus.to_play();
                    }
                    _ => {}
                })
                .children((0..untracked(list).len().min(HERO_COUNT) as u32).map(move |at| {
                    // A view with a button's role, not `button`: the item changes when the dot
                    // gets the focus, which only a view reports.
                    view()
                        .focusable(true)
                        .role(AccessibilityRole::Button)
                        .label(format!("Item {}", at + 1))
                        .size(px(20.0))
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .on_focus(move || index.set(at))
                        .on_press(move || index.set(at))
                        .focus_visible(|s| s.bg(theme().accent).scale(1.25))
                        .child(
                            view()
                                .h(px(6.0))
                                .w(move || px(if index.get() == at { 20.0 } else { 6.0 }))
                                .rounded_full()
                                .bg(move || if index.get() == at { theme().foreground } else { theme().foreground.opacity(0.4) })
                                .transition_all()
                                .duration(motion::BASE)
                                .ease_in_out(),
                        )
                })),
        )
}

/// The dots under the hero, over its bottom on a wide screen; the one of the item in view a pill.
fn dots(list: impl Fn() -> Vec<Media> + Copy + 'static) -> impl IntoElement {
    carousel_pagination()
        .accessibility_label("Choose an item")
        .flex_row()
        .self_center()
        .items_center()
        .gap(px(6.0))
        .h(px(DOTS_HEIGHT))
        .when(is_wide(), |v| v.absolute().bottom(px(12.0)))
        .child(each(
            move || (0..list().len().min(HERO_COUNT) as u32).collect::<Vec<_>>(),
            |i: &u32| *i,
            |i, _| {
                carousel_pagination_item()
                    .index(i.get_untracked())
                    .accessibility_label(format!("Item {}", i.get_untracked() + 1))
                    .h(px(6.0))
                    .w(px(6.0))
                    .rounded_full()
                    .bg(theme().foreground.opacity(0.4))
                    .transition_all()
                    .duration(motion::BASE)
                    .ease_in_out()
                    .selected_style(|s| s.w(px(20.0)).bg(theme().foreground))
            },
        ))
}

