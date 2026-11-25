//! What the home screen is narrowed to: films, shows or music (Remux's `HomeFilter`, `HomeMenu`,
//! `HomeFilters`). On a phone the chips fold out of a glass button at the top left.

use nsh_base::{button, theme};
use nsh_core::prelude::*;
use nsh_icon::labeled_icon;
use nsh_icon_phosphor::Phosphor;

use crate::hooks::server;
use crate::media::MediaType;
use crate::server as api;
use crate::ui::{self, tokens::motion};

/// The kind chosen, if any: `Album` stands for music.
#[derive(Clone, Default, PartialEq)]
struct HomeFilter(Option<MediaType>);

fn filter() -> Signal<HomeFilter> {
    global::<HomeFilter>()
}

/// The kind chosen (tracked where it is read).
fn chosen() -> Option<MediaType> {
    filter().with(|filter| filter.0.clone())
}

/// Whether the Music chip is on (tracked where it is read).
pub fn music() -> bool {
    chosen() == Some(MediaType::Album)
}

/// What the rows and the hero are asked for: the chosen kind, else films and shows; music is
/// albums and artists (tracked where it is read).
pub fn home_types() -> Vec<MediaType> {
    match chosen() {
        Some(MediaType::Album) => vec![MediaType::Album, MediaType::MusicArtist],
        Some(kind) => vec![kind],
        None => vec![MediaType::Movie, MediaType::Series],
    }
}

/// The chips: a chip pressed again is let go.
fn chips() -> impl IntoElement {
    let has_music = resource(|| Some(()), |(), _| async { api::get_music_cached(server()).await.map_err(|error| error.to_string()) });
    let choose = |kind: &'static str| {
        let kind = match kind {
            "movies" => Some(MediaType::Movie),
            "shows" => Some(MediaType::Series),
            "music" => Some(MediaType::Album),
            _ => None,
        };
        filter().update(|filter| filter.0 = if filter.0 == kind { None } else { kind });
    };
    let selected = || match chosen() {
        Some(MediaType::Movie) => "movies",
        Some(MediaType::Series) => "shows",
        Some(MediaType::Album) => "music",
        _ => "",
    };
    dynamic(move || {
        let mut options = vec![("movies", "Films"), ("shows", "Shows")];
        if has_music.get().unwrap_or(false) {
            options.push(("music", "Music"));
        }
        untracked(move || ui::chips(options, selected, choose).into_element())
    })
}

/// The glass button at the top left of a phone's home screen, with the chips folded out of it.
pub fn home_menu() -> impl IntoElement {
    let expanded = signal(false);
    view()
        .absolute()
        .top_0()
        .left_0()
        .pt_safe_area_plus(px(16.0))
        .pl(px(24.0))
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            ui::glass_surface().rounded_full().p_1().child(
                button()
                    .size(px(36.0))
                    .rounded_full()
                    .items_center()
                    .justify_center()
                    .text_color(theme().foreground)
                    .selected(move || expanded.get())
                    .selected_style(|s| s.bg(theme().accent))
                    .transition_colors()
                    .duration(motion::INSTANT)
                    .accessibility_label("Filters")
                    .on_press(move || expanded.update(|open| *open = !*open))
                    .child(labeled_icon(Phosphor::SlidersHorizontal, "Filters").size(px(20.0)))
                    // Something is filtered, and the chips are folded away.
                    .child(when(
                        move || chosen().is_some() && !expanded.get(),
                        || view().absolute().top(px(4.0)).right(px(4.0)).size(px(8.0)).rounded_full().bg(theme().primary),
                    )),
            ),
        )
        .child(when(move || expanded.get(), chips))
}
