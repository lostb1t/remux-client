//! The Media place and what opens from it (Remux's `MediaView`, `LibraryView`, `CollectionView`,
//! `FavoritesView`, `PlaylistsView`, `PersonView`).

use nsh_base::{theme};
use nsh_core::prelude::*;
use uuid::Uuid;

use crate::components::{media_grid, page_header};
use crate::hooks::server;
use crate::media::{CatalogId, Media, MediaType, Sort};
use crate::nav::{self, Route};
use crate::server::{self as api, MediaQuery};
use crate::ui::{self, tokens::{favorites_tint, is_tv, is_wide, motion, playlists_tint}};

/// A page of its own, without the system's header: its content draws one.
fn page(content: impl IntoElement) -> AnyElement {
    crate::stack::page(view().flex_1().bg(theme().background).child(content))
}

/// A page opened over the place: Back with a swipe from the left edge.
fn pushed(content: impl IntoElement) -> AnyElement {
    page(content)
}

/// The side padding of a page.
fn inset() -> f32 {
    if is_wide() { 24.0 } else { 16.0 }
}

/// A banner: a picture, or a tint, with a name over its bottom.
fn banner(name: String, picture: Option<String>, tint: Option<AdaptiveColor>, width: f32, on_press: impl Fn() + 'static) -> View {
    // Back comes back to the tile that opened the page.
    let me = element_ref();
    // The first (Favorites) is where the remote starts on Media (`tv_focus::to_first`).
    if name == "Favorites" {
        crate::tv_focus::set_first_view(crate::nav::Place::Media, me);
    }
    let on_press = move || {
        crate::tv_focus::pressed(me);
        on_press();
    };
    view()
        .element_ref(&me)
        .focusable(true)
        .role(AccessibilityRole::Button)
        .label(name.clone())
        .w(px(width))
        .aspect_ratio(16.0 / 9.0)
        .rounded_lg()
        .overflow_hidden()
        .bg(theme().card)
        .transition_transform()
        .duration(motion::FAST)
        .active(|s| s.scale(motion::PRESS_SCALE))
        .focus_visible(move |s| s.scale(if is_tv() { 1.05 } else { 1.0 }).outline(px(3.0)).outline_offset(px(3.0)).outline_color(theme().ring))
        .on_press(on_press)
        .child(picture.map(|url| image(url).absolute().inset_0().object_fit(ResizeMode::Cover)))
        .child(tint.map(|tint| view().absolute().inset_0().bg_linear_to_br().gradient_from(tint).gradient_to(theme().card)))
        .child(
            view()
                .absolute()
                .inset_0()
                .justify_end()
                .p_4()
                .bg_linear_to_t()
                .gradient_from(theme().background.opacity(0.6))
                .gradient_to(theme().background.opacity(0.0))
                .child(text(name).text_lg().font_weight(FontWeight::SEMIBOLD).text_color(theme().foreground)),
        )
}

/// The Media place: Favorites, Playlists and the server's libraries, as banners.
pub fn media() -> AnyElement {
    let libraries = resource(|| Some(crate::refresh::pulls()), |_, _| async { api::get_libraries_cached(server()).await.map_err(|error| error.to_string()) });
    let columns = if is_wide() { 3.0 } else if window().width.0 >= 640.0 { 2.0 } else { 1.0 };
    let width = (window().width.0 - 2.0 * inset() - (columns - 1.0) * 16.0) / columns;
    let scroll_ref = element_ref();
    page(
        crate::refresh::refreshable(scroll()).element_ref(&scroll_ref).on_scroll(crate::tv_focus::root_scroll(crate::nav::Place::Media, scroll_ref)).flex_1().child(
            view()
                .px(px(inset()))
                .pb(crate::ui::tokens::bottom_room(140.0))
                .child(page_header("Media", false))
                .child(dynamic(move || {
                    let Some(libraries) = libraries.get() else {
                        return match libraries.error() {
                            Some(error) => ui::empty(format!("Could not load the libraries: {error}")).into_element(),
                            None => view()
                                .flex_row()
                                .flex_wrap()
                                .gap_4()
                                .children((0..4).map(move |_| ui::skeleton_box(px(width).into(), px(width * 9.0 / 16.0).into())))
                                .into_element(),
                        };
                    };
                    let server = server();
                    let mut banners = vec![
                        banner("Favorites".into(), None, Some(favorites_tint()), width, || nav::push(Route::Favorites)),
                        banner("Playlists".into(), None, Some(playlists_tint()), width, || nav::push(Route::Playlists)),
                    ];
                    for library in libraries.into_iter().filter(|library| !library.live_tv) {
                        let picture = server.library_image_url(&library);
                        let (id, name) = (library.id, library.name.clone());
                        banners.push(banner(library.name, picture, None, width, move || nav::push(Route::Library { id, name: name.clone() })));
                    }
                    view().flex_row().flex_wrap().gap_4().children(banners).into_element()
                })),
        ),
    )
}

/// A grid page: its header, and the grid of `query`.
fn grid_page(title: String, query: MediaQuery, caption: bool) -> AnyElement {
    // No side padding of its own: a grid's header has the grid's.
    pushed(media_grid(page_header(title, true), query, caption))
}

/// What is in one library; a library of collections has them to show.
pub fn library(id: Uuid, name: String) -> AnyElement {
    let query = MediaQuery::builder().parent(id).types(vec![MediaType::Movie, MediaType::Series, MediaType::Collection]).build();
    grid_page(name, query, false)
}

/// What is in a collection: its own items, not those of the collections in it.
pub fn collection(id: Uuid, name: String) -> AnyElement {
    let query = MediaQuery::builder()
        .parent(id)
        .types(vec![MediaType::Movie, MediaType::Series, MediaType::Collection])
        .recursive(false)
        .build();
    grid_page(name, query, false)
}

/// What the user marked as a favorite.
pub fn favorites() -> AnyElement {
    grid_page("Favorites".into(), MediaQuery::builder().catalog(CatalogId::Favorites).build(), false)
}

/// The user's playlists: the name, and how many items are in it.
pub fn playlists() -> AnyElement {
    let lists = resource(|| Some(()), |(), _| async { server().playlists_with_counts().await.map_err(|error| error.to_string()) });
    pushed(
        scroll().flex_1().child(
            view().px(px(inset())).pb(crate::ui::tokens::bottom_room(140.0)).gap_4().child(page_header("Playlists", true)).child(dynamic(move || {
                match (lists.get(), lists.error()) {
                    (_, Some(error)) => ui::empty(format!("Could not load the playlists: {error}")).into_element(),
                    (None, _) => view().items_center().py_6().child(ui::spinner()).into_element(),
                    (Some(list), _) if list.is_empty() => {
                        ui::empty("No playlists yet. Add something to one from the menu of a title or a track.").into_element()
                    }
                    (Some(list), _) => ui::menu_group(
                        list.into_iter()
                            .map(|(id, name, count)| {
                                let label = format!("{name} · {count} {}", if count == 1 { "item" } else { "items" });
                                ui::menu_row(None, label, true, move || nav::push(Route::Playlist { id, name: name.clone() }))
                            })
                            .collect(),
                    )
                    .into_element(),
                }
            })),
        ),
    )
}

/// Who a person is and what they are in.
pub fn person(id: Uuid, name: String) -> AnyElement {
    let query = MediaQuery::builder().person(id).types(vec![MediaType::Movie, MediaType::Series]).sort(Sort::Released).descending(true).build();
    let header = view().child(page_header(name.clone(), true)).child(person_header(id, name)).child(
        text("Appears in").pb_3().text_xl().font_weight(FontWeight::SEMIBOLD).text_color(theme().foreground),
    );
    pushed(media_grid(header, query, true))
}

/// A person's portrait, when and where they were born, and their story (Remux's `PersonHeader`).
fn person_header(id: Uuid, name: String) -> impl IntoElement {
    let details = resource(|| Some(()), move |(), _| async move { Ok::<_, String>(server().media_details(id).await.ok().flatten()) });
    let initial = name.chars().next().map(String::from).unwrap_or_default();
    view().pb_8().child(dynamic(move || {
        let person: Option<Media> = details.get().flatten();
        let initial = initial.clone();
        untracked(move || {
            let server = server();
            let portrait = person.as_ref().and_then(|person| server.portrait_url(person));
            let date = |date: chrono::DateTime<chrono::Utc>| date.format("%-d %B %Y").to_string();
            let place = person.as_ref().and_then(|person| person.production_locations.first().cloned());
            let born = match (person.as_ref().and_then(|person| person.release_date).map(date), place) {
                (Some(born), Some(place)) => Some(format!("Born {born} in {place}")),
                (Some(born), None) => Some(format!("Born {born}")),
                (None, Some(place)) => Some(format!("Born in {place}")),
                (None, None) => None,
            };
            let died = person.as_ref().and_then(|person| person.end).map(|died| format!("Died {}", date(died)));
            let story = person.as_ref().and_then(|person| person.description.clone()).filter(|story| !story.is_empty());
            let width = if is_wide() { 160.0 } else { 112.0 };
            view()
                .gap_4()
                .child(
                    view()
                        .flex_row()
                        .gap_4()
                        .child(
                            view()
                                .w(px(width))
                                .aspect_ratio(2.0 / 3.0)
                                .rounded_xl()
                                .overflow_hidden()
                                .bg(theme().card)
                                .items_center()
                                .justify_center()
                                .child(match portrait {
                                    Some(url) => image(url).size_full().object_fit(ResizeMode::Cover).into_element(),
                                    None => text(initial).text_4xl().font_weight(FontWeight::SEMIBOLD).text_color(theme().muted_foreground).into_element(),
                                }),
                        )
                        .child(
                            view()
                                .flex_1()
                                .self_center()
                                .gap_2()
                                .child(born.map(|born| text(born).text_sm().text_color(theme().muted_foreground)))
                                .child(died.map(|died| text(died).text_sm().text_color(theme().muted_foreground))),
                        ),
                )
                .child(story.map(ui::read_more))
                .into_element()
        })
    }))
}
