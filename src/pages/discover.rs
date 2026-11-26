//! Discover (Remux's `Discover`, `SearchBox`): everything there is, narrowed by kind and genre and
//! ordered; or what a search found, a row for each kind. What is chosen is kept while the place is
//! left and come back to.

use nsh_base::{button, theme};
use nsh_core::prelude::*;
use nsh_icon_phosphor::Phosphor;

use crate::components::{media_grid, media_row, page_header};
use crate::hooks::server;
use crate::media::{CardVariant, Genre, MediaType, Sort};
use crate::nav;
use crate::server::{self as api, MediaQuery};
use crate::ui::{self, tokens::{glass, is_wide, motion}};

/// The kinds of thing Discover looks for, and what each is called.
const ALL_TYPES: [(MediaType, &str); 8] = [
    (MediaType::Movie, "Movies"),
    (MediaType::Series, "Shows"),
    (MediaType::Audio, "Music"),
    (MediaType::Album, "Albums"),
    (MediaType::MusicArtist, "Artists"),
    (MediaType::Person, "People"),
    (MediaType::Channel, "Channels"),
    (MediaType::Program, "Programs"),
];

/// What Discover is narrowed and ordered by, and what is searched for.
#[derive(Clone, PartialEq)]
struct Filter {
    /// What is looked for: all there is when none of what is offered is chosen.
    types: Vec<MediaType>,
    genre: Option<Genre>,
    sort: Sort,
    descending: bool,
    search: String,
}

impl Default for Filter {
    fn default() -> Self {
        Self { types: Vec::new(), genre: None, sort: Sort::Popular, descending: true, search: String::new() }
    }
}

fn filter() -> Signal<Filter> {
    global::<Filter>()
}

/// The side padding of the page.
fn inset() -> f32 {
    if is_wide() { 24.0 } else { 16.0 }
}

/// The Discover place.
pub fn discover() -> AnyElement {
    // The rows of a search are the remote's to go between (`tv_focus`).
    let focus = crate::tv_focus::Scope::provide();
    // A search on a TV: the remote goes to what was found, once it is there (the keyboard goes
    // with the field's focus), as in Remux. Here, not in the header, which a search makes again.
    let searched = signal(0u32);
    effect(move || {
        if searched.get() == 0 || !crate::ui::tokens::is_tv() {
            return;
        }
        spawn(async move {
            // After the page is made again for the search (its field, made anew, takes the focus
            // back for a moment), and once a row of what was found is there.
            timer(std::time::Duration::from_millis(1000)).await;
            for _ in 0..30 {
                if focus.to_row_after(None) {
                    break;
                }
                timer(std::time::Duration::from_millis(300)).await;
            }
        });
    });
    // Live TV only for who has it; Popular and Trending are a Remux server's.
    let live_tv = resource(|| Some(()), |(), _| async { api::get_live_tv_cached(server()).await.map_err(|error| error.to_string()) });
    let remux = resource(|| Some(()), |(), _| async { Ok::<_, String>(server().is_remux().await) });
    let types_open = signal(false);
    let scroll_ref = element_ref();
    let genres_open = signal(false);
    let sort_open = signal(false);
    let offered = move || {
        let live = live_tv.get().unwrap_or(false);
        ALL_TYPES.iter().filter(|(kind, _)| live || !matches!(kind, MediaType::Channel | MediaType::Program)).cloned().collect::<Vec<_>>()
    };
    // What is chosen of what is offered: nothing chosen is all of it.
    let chosen = move || {
        let offered = offered();
        let picked = filter().with(|filter| filter.types.clone());
        let kept: Vec<MediaType> = offered.iter().map(|(kind, _)| kind.clone()).filter(|kind| picked.contains(kind)).collect();
        if kept.is_empty() { offered.into_iter().map(|(kind, _)| kind).collect() } else { kept }
    };
    let sort = move || {
        let sort = filter().with(|filter| filter.sort);
        if remux.get() == Some(false) && sort.remux_only() { Sort::CommunityRating } else { sort }
    };

    crate::stack::page(
        view()
            .flex_1()
            .bg(theme().background)
            .child(dynamic(move || {
                // Nothing is asked for until it is known what the server offers.
                let known = remux.get().is_some() && (live_tv.get().is_some() || live_tv.error().is_some());
                if !known {
                    return view().flex_1().items_center().justify_center().child(ui::spinner()).into_element();
                }
                let state = filter().get();
                // Made again when it is pulled down to refresh.
                crate::refresh::pulls();
                let chosen = chosen();
                let offered = offered();
                let sort = sort();
                untracked(move || {
                    let header = header(chosen.clone(), offered.clone(), &state, sort, types_open, genres_open, sort_open, searched);
                    let searching = !state.search.trim().is_empty();
                    let genres: Vec<String> = state.genre.iter().map(|genre| genre.name.clone()).collect();
                    if !searching {
                        let query = MediaQuery::builder()
                            .types(chosen)
                            .genres(genres)
                            .descending(state.descending)
                            .sort(sort)
                            .build();
                        media_grid(header, query, true).into_element()
                    } else {
                        // A row for each kind: the kinds are looked for in different places, have
                        // different pictures, and cannot be put in one order.
                        crate::refresh::refreshable(scroll())
                            .element_ref(&scroll_ref)
                            .on_scroll(crate::tv_focus::root_scroll(crate::nav::Place::Discover, scroll_ref))
                            .flex_1()
                            .child(view().pb(crate::ui::tokens::bottom_room(140.0)).child(view().px(px(inset())).child(header)).child(view().gap_6().children(offered.into_iter().enumerate().map(
                                |(at, (kind, name))| {
                                    let films = matches!(kind, MediaType::Movie | MediaType::Series);
                                    let query = MediaQuery::builder()
                                        .types(vec![kind.clone()])
                                        .genres(if films { genres.clone() } else { Vec::new() })
                                        .search_query(state.search.trim().to_owned())
                                        .limit(20)
                                        .build();
                                    let variant = match kind {
                                        MediaType::Channel | MediaType::Program => CardVariant::Landscape,
                                        MediaType::Audio | MediaType::Album | MediaType::MusicArtist => CardVariant::Square,
                                        _ => CardVariant::Poster,
                                    };
                                    media_row(name.to_owned(), query, variant, Some(at), nav::open_media)
                                },
                            ))))
                            .into_element()
                    }
                })
            }))
            .child(types_sheet(types_open, chosen, offered))
            .child(ui::sheet(genres_open, "Genres", move || genre_list(genres_open)))
            .child(ui::sheet(sort_open, "Sort by", move || sort_list(sort_open, remux.get().unwrap_or(false), sort()))),
    )
}

/// A chip of the filter row: what it says, chosen or not.
fn chip(label: impl Into<SharedString>, active: bool, on_press: impl Fn() + 'static) -> nsh_base::ButtonElement {
    let label: SharedString = label.into();
    button()
        .h(px(34.0))
        .px_4()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(if active { theme().accent } else { glass() })
        .border_1()
        .border_color(theme().border)
        .transition_transform()
        .duration(motion::FAST)
        .active(|s| s.scale(motion::PRESS_SCALE))
        .focus_visible(|s| s.bg(theme().primary))
        .accessibility_label(label.clone())
        .on_press(on_press)
        .child(text(label).text_sm().font_weight(FontWeight::MEDIUM).text_color(theme().foreground))
}

/// The top of the page: its title, the search field, and the chips.
fn header(
    chosen: Vec<MediaType>,
    offered: Vec<(MediaType, &'static str)>,
    state: &Filter,
    sort: Sort,
    types_open: Signal<bool>,
    genres_open: Signal<bool>,
    sort_open: Signal<bool>,
    searched: Signal<u32>,
) -> impl IntoElement {
    let searching = !state.search.trim().is_empty();
    let draft = signal(state.search.clone());
    let types_label = if chosen.len() == offered.len() {
        "All types".to_owned()
    } else if chosen.len() == 1 {
        offered.iter().find(|(kind, _)| *kind == chosen[0]).map(|(_, name)| (*name).to_owned()).unwrap_or_default()
    } else {
        format!("{} types", chosen.len())
    };
    let descending = state.descending;
    // Up from the first row of a search goes to the genre's chip, which is always there.
    let focus = crate::tv_focus::Scope::current();
    let genre_chip = element_ref();
    focus.set_top(genre_chip);
    // The remote starts on the search field (`tv_focus::to_first`).
    let search_field = element_ref();
    crate::tv_focus::set_first_view(crate::nav::Place::Discover, search_field);
    // No side padding of its own: a grid's header has the grid's.
    view()
        .gap_4()
        .pb_4()
        .child(page_header("Discover", false))
        .child({
            let submit = move |query: &str| {
                let query = query.trim().to_owned();
                filter().update(|filter| filter.search = query);
                searched.update(|count| *count += 1);
            };
            if crate::ui::tokens::is_tv() {
                tv_search(draft, submit, search_field).into_element()
            } else {
                ui::field_named(ui::FieldKind::Text, draft, "Search", submit, search_field).into_element()
            }
        })
        .child(
            scroll().horizontal().scrollbar(false).child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    // A search shows every kind, a row of its own: there is nothing to choose.
                    .child((!searching).then(|| chip(types_label, chosen.len() != offered.len(), move || types_open.set(true))))
                    .child(match state.genre.clone() {
                        Some(genre) => view()
                            .flex_row()
                            .gap_1()
                            .child(chip(genre.name, true, move || genres_open.set(true)).element_ref(&genre_chip))
                            .child(ui::icon_button(ui::Size::Regular, Phosphor::X, "Clear genre", || filter().update(|filter| filter.genre = None)))
                            .into_element(),
                        None => chip("Genre", false, move || genres_open.set(true)).element_ref(&genre_chip).into_element(),
                    })
                    .child(searching.then(|| chip("Clear search", false, || filter().update(|filter| filter.search.clear()))))
                    // Which way a search goes is up to the server.
                    .child((!searching).then(|| chip(sort.label(), false, move || sort_open.set(true))))
                    .child((!searching).then(|| {
                        ui::icon_button(
                            ui::Size::Regular,
                            if descending { Phosphor::SortDescending } else { Phosphor::SortAscending },
                            if descending { "Descending" } else { "Ascending" },
                            move || filter().update(|filter| filter.descending = !filter.descending),
                        )
                    })),
            ),
        )
}

/// The kinds to choose: added to what is chosen, or taken from it; the last one cannot go.
fn types_sheet(
    open: Signal<bool>,
    chosen: impl Fn() -> Vec<MediaType> + Copy + 'static,
    offered: impl Fn() -> Vec<(MediaType, &'static str)> + Copy + 'static,
) -> impl IntoElement {
    ui::sheet(open, "Types", move || {
        dynamic(move || {
            let picked = chosen();
            ui::menu_group(
                offered()
                    .into_iter()
                    .map(|(kind, name)| {
                        let on = picked.contains(&kind);
                        let picked = picked.clone();
                        let row = ui::menu_row(on.then_some(Phosphor::Check), name, false, move || {
                            let mut next = picked.clone();
                            if next.contains(&kind) {
                                next.retain(|other| *other != kind);
                            } else {
                                next.push(kind.clone());
                            }
                            if !next.is_empty() {
                                filter().update(|filter| filter.types = next);
                            }
                        });
                        row
                    })
                    .collect(),
            )
            .into_element()
        })
    })
}

/// The server's genres, to narrow to one (Remux's `GenreList`).
fn genre_list(open: Signal<bool>) -> impl IntoElement {
    let genres = resource(|| Some(()), |(), _| async { server().genres().await.map_err(|error| error.to_string()) });
    dynamic(move || match (genres.get(), genres.error()) {
        (_, Some(error)) => ui::menu_note(format!("Could not load the genres: {error}")).into_element(),
        (None, _) => view().items_center().py_6().child(ui::spinner()).into_element(),
        (Some(list), _) => ui::menu_group(
            list.into_iter()
                .map(|genre| {
                    let name = genre.name.clone();
                    ui::menu_row(None, name, false, move || {
                        filter().update(|filter| filter.genre = Some(genre.clone()));
                        open.set(false);
                    })
                })
                .collect(),
        )
        .into_element(),
    })
}

/// The orders to choose; Popular and Trending only on a Remux server.
fn sort_list(open: Signal<bool>, remux: bool, current: Sort) -> impl IntoElement {
    ui::menu_group(
        Sort::ALL
            .into_iter()
            .filter(|sort| remux || !sort.remux_only())
            .map(|choice| {
                ui::menu_row((choice == current).then_some(Phosphor::Check), choice.label(), false, move || {
                    filter().update(|filter| {
                        filter.sort = choice;
                        filter.descending = choice.descending_first();
                    });
                    open.set(false);
                })
            })
            .collect(),
    )
}

/// The search on a TV: a button in the field's shape, which the remote can stand on (a text field
/// with the focus opens the keyboard at once, which takes the remote's keys); OK opens the field,
/// with the keyboard, and sending it closes it again. `me` names the button.
fn tv_search(draft: Signal<String>, on_submit: impl Fn(&str) + Copy + 'static, me: ElementRef) -> impl IntoElement {
    let editing = signal(false);
    let field = element_ref();
    view().child(
        when(move || editing.get(), move || {
            // The field, focused once it is there.
            set_timeout(std::time::Duration::from_millis(50), move || field.focus());
            ui::field_named(ui::FieldKind::Text, draft, "Search", move |query: &str| {
                editing.set(false);
                on_submit(query);
            }, field)
        })
        .otherwise(move || {
            view()
                .element_ref(&me)
                .focusable(true)
                .role(AccessibilityRole::Button)
                .label("Search")
                .flex_row()
                .items_center()
                .gap_3()
                .px_4()
                .py(px(14.0))
                .rounded_full()
                .bg(crate::ui::tokens::group())
                .focus_visible(|s| s.outline(px(3.0)).outline_offset(px(3.0)).outline_color(theme().ring))
                .on_press(move || editing.set(true))
                .child(nsh_icon::icon(Phosphor::MagnifyingGlass).size(px(20.0)).text_color(theme().muted_foreground))
                .child(
                    text(move || {
                        let now = draft.get();
                        SharedString::from(if now.is_empty() { "Search".to_owned() } else { now })
                    })
                    .text_size(px(17.0))
                    .text_color(move || if draft.with(String::is_empty) { theme().muted_foreground } else { theme().foreground }),
                )
        }),
    )
}
