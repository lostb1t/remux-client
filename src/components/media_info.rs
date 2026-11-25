//! What a title's page tells about it (Remux's `MediaInfo`, `TagsDisplay`, `ExternalLinks`,
//! `CastRow`): the story and the facts around it, its pages elsewhere, and its cast.

use nsh_base::{link, theme};
use nsh_core::prelude::*;

use crate::hooks::server;
use crate::media::{Media, MediaType, PersonKind};
use crate::nav::{self, Route};
use crate::tv_focus;
use crate::ui::tokens::{group, is_tv, is_wide, motion, secondary_text};

/// Distinct names of the people of one kind, in the order they appear in.
fn names_of(item: &Media, kind: PersonKind) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for person in item.people_of(kind) {
        if !names.contains(&person.name) {
            names.push(person.name.clone());
        }
    }
    names
}

fn plural(count: usize, noun: &str) -> String {
    if count > 1 { format!("{noun}s") } else { noun.to_string() }
}

/// A fact: what it is, and its value.
fn fact(label: String, value: String) -> View {
    view()
        .gap_1()
        .min_w(px(140.0))
        .flex_1()
        .child(text(label.to_uppercase()).text_xs().text_color(theme().muted_foreground))
        .child(text(value).text_sm().text_color(theme().foreground))
}

/// A round label (a genre, a link).
fn pill(label: String) -> View {
    view().px_3().py_1().rounded_full().bg(theme().accent).child(text(label).text_xs().font_weight(FontWeight::MEDIUM).text_color(theme().foreground))
}

/// Where else a title is on the web: IMDb, TMDb, Letterboxd, Trakt, a search at Rotten Tomatoes.
/// A TV has no browser for them, so it shows none.
fn external_links(item: &Media) -> Option<View> {
    if is_tv() {
        return None;
    }
    let mut links: Vec<(&'static str, String)> = Vec::new();
    let imdb = item.external_ids.imdb.as_deref().filter(|id| !id.is_empty());
    if let Some(imdb) = imdb {
        links.push(("IMDb", format!("https://www.imdb.com/title/{imdb}/")));
    }
    // An episode's id at TMDb is not one that has a page of its own.
    if let Some(tmdb) = item.external_ids.tmdb {
        match item.media_type {
            MediaType::Movie => {
                links.push(("TMDb", format!("https://www.themoviedb.org/movie/{tmdb}")));
                links.push(("Letterboxd", format!("https://letterboxd.com/tmdb/{tmdb}")));
            }
            MediaType::Series => links.push(("TMDb", format!("https://www.themoviedb.org/tv/{tmdb}"))),
            _ => {}
        }
    }
    // Trakt takes the id of IMDb in the place of its own slug.
    if let (Some(imdb), Some(section)) = (imdb, match item.media_type {
        MediaType::Movie => Some("movies"),
        MediaType::Series => Some("shows"),
        _ => None,
    }) {
        links.push(("Trakt", format!("https://trakt.tv/{section}/{imdb}")));
    }
    if matches!(item.media_type, MediaType::Movie | MediaType::Series) {
        let title: String = url::form_urlencoded::byte_serialize(item.title.as_bytes()).collect();
        links.push(("Rotten Tomatoes", format!("https://www.rottentomatoes.com/search?search={title}")));
    }
    (!links.is_empty()).then(|| {
        view().flex_row().flex_wrap().gap_2().children(links.into_iter().map(|(label, href)| {
            link()
                .href(href)
                .px_3()
                .py_1()
                .rounded_full()
                .bg(theme().accent)
                .transition_colors()
                .duration(motion::INSTANT)
                .hover(|s| s.bg(theme().foreground.opacity(0.2)))
                .active(|s| s.bg(theme().foreground.opacity(0.2)))
                .focus_visible(|s| s.bg(theme().primary))
                .child(text(label).text_xs().font_weight(FontWeight::MEDIUM).text_color(theme().foreground))
        }))
    })
}

/// The story and the facts around `item`: beside each other on a wide screen.
pub fn media_info(item: &Media) -> impl IntoElement {
    let directors = names_of(item, PersonKind::Director);
    let writers = names_of(item, PersonKind::Writer);
    let released = item.release_date.map(|date| date.format("%-d %B %Y").to_string());
    let mut tags = Vec::new();
    tags.extend(item.official_rating.clone());
    if item.runtime_seconds.is_some() {
        tags.push(item.formatted_runtime());
    }
    let mut facts = Vec::new();
    facts.extend(released.map(|released| fact("Released".into(), released)));
    if !directors.is_empty() {
        facts.push(fact(plural(directors.len(), "Director"), directors.join(", ")));
    }
    if !writers.is_empty() {
        facts.push(fact(plural(writers.len(), "Writer"), writers.join(", ")));
    }
    if !item.studios.is_empty() {
        facts.push(fact(plural(item.studios.len(), "Studio"), item.studios.join(", ")));
    }
    let wide = is_wide();
    view()
        .px_6()
        .pt_6()
        .gap_8()
        .when(wide, |v| v.flex_row())
        .child(
            view()
                .gap_4()
                .when(wide, |v| v.flex_grow(2.0).basis(px(0.0)))
                .child(item.tagline.clone().map(|tagline| text(tagline).text_base().italic().text_color(secondary_text())))
                .child(item.description.clone().map(|story| text(story).text_base().line_height(px(26.0)).text_color(theme().foreground)))
                .child((!tags.is_empty()).then(|| {
                    view().flex_row().gap_2().children(tags.into_iter().map(|tag| text(tag).text_sm().font_weight(FontWeight::MEDIUM).text_color(theme().foreground)))
                }))
                .child((!item.genres.is_empty()).then(|| view().flex_row().flex_wrap().gap_2().children(item.genres.iter().cloned().map(pill))))
                .child(external_links(item)),
        )
        .child((!facts.is_empty()).then(|| {
            view().gap_4().when(wide, |v| v.flex_1()).when(!wide, |v| v.flex_row().flex_wrap().gap_x(px(24.0))).children(facts)
        }))
}

/// How many of the cast to show; the rest is mostly bit parts.
const CAST_LIMIT: usize = 30;

/// The cast of `item`: round portraits, the name and the part, opening the person's page.
/// `place` is the row's place on the page, for the remote.
pub fn cast_row(item: &Media, place: usize) -> Option<impl IntoElement> {
    let cast: Vec<_> = item.people_of(PersonKind::Actor).take(CAST_LIMIT).cloned().collect();
    if cast.is_empty() {
        return None;
    }
    let focus = tv_focus::Scope::current();
    let server = server();
    Some(
        view()
            .pt_8()
            .gap_3()
            .child(text("Cast").px_6().text_xl().font_weight(FontWeight::SEMIBOLD).text_color(theme().foreground))
            .child(
                scroll().horizontal().scrollbar(false).child(view().flex_row().gap_4().px_6().py_3().children(cast.into_iter().enumerate().map(move |(at, person)| {
                    let portrait = server.person_image_url(&person);
                    let initial = person.name.chars().next().map(String::from).unwrap_or_default();
                    let (id, name) = (person.id, person.name.clone());
                    let card = view()
                        .focusable(true)
                        .role(AccessibilityRole::Button)
                        .label(person.name.clone())
                        .w(px(96.0))
                        .items_center()
                        .gap_1()
                        .rounded_lg()
                        .transition_transform()
                        .duration(motion::FAST)
                        .active(|s| s.scale(motion::PRESS_SCALE))
                        .focus_visible(|s| s.scale(1.08))
                        .on_press(move || nav::push(Route::Person { id, name: name.clone() }))
                        .child(
                            view()
                                .size(px(96.0))
                                .rounded_full()
                                .overflow_hidden()
                                .bg(group())
                                .items_center()
                                .justify_center()
                                .child(match portrait {
                                    Some(url) => image(url).size_full().object_fit(ResizeMode::Cover).into_element(),
                                    None => text(initial).text_2xl().font_weight(FontWeight::SEMIBOLD).text_color(theme().muted_foreground).into_element(),
                                }),
                        )
                        .child(text(person.name.clone()).mt_1().text_sm().font_weight(FontWeight::MEDIUM).number_of_lines(1).text_center().text_color(theme().foreground))
                        .child(person.role.clone().map(|role| text(role).text_xs().number_of_lines(1).text_center().text_color(theme().muted_foreground)));
                    if is_tv() && at == 0 {
                        let target = element_ref();
                        focus.set_row(place, target);
                        card.element_ref(&target)
                    } else {
                        card
                    }
                }))),
            )
            .when(is_tv(), move |v| {
                v.capture_keys(tv_focus::capture(&["up", "down"])).on_key_down(move |event: &nsh_core::keyboard::KeyEvent| match event.key.as_ref() {
                    "ArrowDown" => {
                        focus.to_row_after(Some(place));
                    }
                    "ArrowUp" => {
                        focus.to_row_before(place);
                    }
                    _ => {}
                })
            }),
    )
}
