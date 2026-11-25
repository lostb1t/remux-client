//! A show's seasons and the episodes of one (Remux's `MediaDetailSeason`, `SeasonEpisodes`): it
//! opens at the season the viewer is in, the one of the episode that is next.

use nsh_base::theme;
use nsh_core::keyboard::KeyEvent;
use nsh_core::prelude::*;
use uuid::Uuid;

use crate::hooks::server;
use crate::media::{Media, MediaType};
use crate::nav::{self, Route};
use crate::server::MediaQuery;
use crate::tv_focus;
use crate::ui::{self, tokens::{group, is_tv, motion, track}};

/// An episode card's width.
const EPISODE_WIDTH: f32 = 220.0;

/// The seasons of `series` and the episodes of the chosen one; `place` is its place among the
/// rows of the page, for the remote.
pub fn seasons(series: Media, place: usize) -> impl IntoElement {
    let series_id = series.id;
    let seasons = resource(|| Some(()), move |(), _| async move {
        // All of them: a long show has more than a page of the default 25 (Remux misses those).
        let query = MediaQuery::builder().parent(series_id).types(vec![MediaType::Season]).limit(200).build();
        server().media(&query).await.map_err(|error| error.to_string())
    });
    let next = resource(|| Some(()), move |(), _| {
        let series = series.clone();
        async move { Ok::<_, String>(server().nextup(&series).await.ok().and_then(|items| items.into_iter().next())) }
    });
    view().pt_6().gap_4().child(dynamic(move || {
        let (Some(list), Some(next)) = (seasons.get(), next.get()) else {
            return match seasons.error() {
                Some(error) => ui::menu_note(format!("Could not load the seasons: {error}")).into_element(),
                None => view().h(px(200.0)).items_center().justify_center().child(ui::spinner()).into_element(),
            };
        };
        if list.is_empty() {
            return view().into_element();
        }
        let current = next.and_then(|episode| episode.season_id).filter(|id| list.iter().any(|season| season.id == *id));
        untracked(move || season_episodes(list, current, place).into_element())
    }))
}

fn season_episodes(seasons: Vec<Media>, current: Option<Uuid>, place: usize) -> impl IntoElement {
    let first = seasons.first().map(|season| season.id);
    let selected = signal(current.or(first).unwrap_or_default());
    let focus = tv_focus::Scope::current();
    let options: Vec<(Uuid, &'static str)> =
        seasons.iter().map(|season| (season.id, &*Box::leak(season.title.clone().into_boxed_str()))).collect();
    view()
        .gap_4()
        .child(scroll().horizontal().scrollbar(false).child(view().px_6().child(ui::chips(options, move || selected.get(), move |season| selected.set(season)))))
        .child(dynamic(move || {
            let season = selected.get();
            untracked(move || episodes(season, place, focus).into_element())
        }))
}

/// The episodes of `season`, in a row.
fn episodes(season: Uuid, place: usize, focus: tv_focus::Scope) -> impl IntoElement {
    let list = resource(
        move || Some(crate::user_data::last().seq),
        move |_, _| async move {
            let query = MediaQuery::builder().parent(season).types(vec![MediaType::Episode]).limit(500).build();
            server().media(&query).await.map_err(|error| error.to_string())
        },
    );
    view()
        .when(is_tv(), move |v| {
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
            scroll().horizontal().scrollbar(false).child(
                view().flex_row().gap_4().px_6().py(px(12.0)).child(each(
                    move || list.get().unwrap_or_default(),
                    |episode: &Media| episode.id,
                    move |episode, at| {
                        let card = episode_card(episode.get_untracked());
                        if is_tv() && at.get_untracked() == 0 {
                            let target = element_ref();
                            focus.set_row(place, target);
                            card.element_ref(&target)
                        } else {
                            card
                        }
                    },
                )),
            ),
        )
}

/// An episode: its still, its number, its title and the start of its story; how far it was
/// watched under the still.
fn episode_card(episode: Media) -> View {
    let still = server().still_url(&episode, (EPISODE_WIDTH * 3.0) as u32);
    let progress = episode.progress();
    let (media_type, id) = (episode.media_type.clone(), episode.id);
    let me = element_ref();
    view()
        .element_ref(&me)
        .focusable(true)
        .role(AccessibilityRole::Button)
        .label(episode.title.clone())
        .w(px(EPISODE_WIDTH))
        .rounded_lg()
        .overflow_hidden()
        .bg(group())
        .transition_transform()
        .duration(motion::FAST)
        .active(|s| s.scale(motion::PRESS_SCALE))
        .focus_visible(move |s| s.scale(if is_tv() { 1.05 } else { 1.0 }).outline(px(3.0)).outline_offset(px(3.0)).outline_color(theme().ring))
        .on_press(move || {
            crate::tv_focus::pressed(me);
            nav::push(Route::MediaDetail { media_type: media_type.clone(), id });
        })
        .on_long_press({
            let episode = episode.clone();
            move |_: &PressEvent| crate::components::open_item_menu(episode.clone())
        })
        .child(
            view()
                .w_full()
                .h(px(EPISODE_WIDTH * 9.0 / 16.0))
                .bg(ui::tokens::skeleton())
                .child(still.map(|url| image(url).size_full().object_fit(ResizeMode::Cover)))
                .child(progress.map(|progress| {
                    view()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(4.0))
                        .bg(track())
                        .child(view().h_full().w(nsh_core::geometry::relative(progress as f32 / 100.0)).bg(theme().primary))
                })),
        )
        .child(
            view()
                .p_4()
                .gap_1()
                .child(episode.index_number.map(|number| {
                    text(format!("EPISODE {number}")).text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme().muted_foreground)
                }))
                .child(text(episode.title.clone()).text_sm().font_weight(FontWeight::SEMIBOLD).number_of_lines(2).text_color(theme().foreground))
                .child(episode.description.clone().map(|story| text(story).text_xs().number_of_lines(4).text_color(theme().muted_foreground))),
        )
}
