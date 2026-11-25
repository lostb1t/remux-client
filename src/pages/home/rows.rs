//! The rows of the home screen: Next Up and the newest of each library, or with the Music chip the
//! albums and artists (Remux: `HomeRows`, `MusicRows`).

use nsh_base::theme;
use nsh_core::prelude::*;

use super::filters::{self, home_types};
use crate::components::{media_row, row_skeletons};
use crate::hooks::server;
use crate::media::{CardVariant, Catalog, LibraryKind, Media, MediaType, Sort};
use crate::server::{self as api, MediaQuery};
use crate::ui::tokens::is_tv;

/// The cards a row asks for at first: a TV shows about seven (Remux's `ROW_PAGE`).
const ROW_PAGE: u32 = 10;

/// Whether a row is on the home screen for the chosen kinds of item: a library goes by what it is
/// for, and a mixed one is always there (Remux's `shown_for`).
fn shown_for(catalog: &Catalog, types: &[MediaType]) -> bool {
    match catalog.library {
        Some(LibraryKind::Movies) => types.contains(&MediaType::Movie),
        Some(LibraryKind::Shows) => types.contains(&MediaType::Series),
        Some(LibraryKind::Mixed) | None => true,
    }
}

/// The rows, under the hero: made again when a filter is chosen.
pub fn rows(on_open: impl Fn(Media) + Copy + 'static) -> impl IntoElement {
    view()
        // On a TV the last row can come up to the middle of the screen too.
        .when(is_tv(), |v| v.pb(px(window().height.0 / 2.0)))
        .pb(crate::ui::tokens::bottom_room(120.0))
        .child(dynamic(move || {
            let music = filters::music();
            let hide_watched = crate::settings::settings().filter_watched;
            untracked(move || if music { music_rows(on_open).into_element() } else { catalog_rows(hide_watched, on_open).into_element() })
        }))
}

/// Next Up and the libraries' rows, of the kinds chosen.
fn catalog_rows(hide_watched: bool, on_open: impl Fn(Media) + Copy + 'static) -> impl IntoElement {
    let catalogs = resource(|| Some(()), |(), _| async { api::get_catalogs_cached(server()).await.map_err(|error| error.to_string()) });
    let types = home_types();
    let shown = {
        let types = types.clone();
        move || catalogs.get().unwrap_or_default().into_iter().filter(|catalog| shown_for(catalog, &types)).collect::<Vec<_>>()
    };
    view()
        .gap_6()
        .child(when(move || catalogs.loading() && catalogs.get().is_none(), row_skeletons))
        .child(when(
            move || catalogs.error().is_some(),
            move || {
                text(move || format!("Could not load the catalogs: {}", catalogs.error().unwrap_or_default()))
                    .px_6()
                    .text_color(theme().muted_foreground)
            },
        ))
        .child(each(shown, |catalog: &Catalog| catalog.id.clone(), move |catalog, at| {
            let catalog = catalog.get_untracked();
            // A mixed library is asked for whatever it has.
            let types = match catalog.library {
                Some(LibraryKind::Mixed) => Vec::new(),
                _ => types.clone(),
            };
            let query = MediaQuery::builder()
                .limit(ROW_PAGE)
                .types(types)
                .catalog(catalog.id.clone())
                .hide_watched(hide_watched)
                .latest(true)
                .build();
            media_row(catalog.title.clone(), query, catalog.card_variant, Some(at.get_untracked()), on_open)
        }))
}

/// The rows for music: the newest and the most played albums, and the artists (Remux's
/// `MusicRows`).
fn music_rows(on_open: impl Fn(Media) + Copy + 'static) -> impl IntoElement {
    let rows = [
        ("Recently added", MediaType::Album, Sort::Added),
        ("Popular albums", MediaType::Album, Sort::Popular),
        ("New releases", MediaType::Album, Sort::Released),
        ("Artists", MediaType::MusicArtist, Sort::Popular),
    ];
    view().gap_6().pt(px(80.0)).children(rows.into_iter().enumerate().map(move |(at, (title, kind, sort))| {
        let query = MediaQuery::builder().limit(15).types(vec![kind]).sort(sort).descending(true).build();
        media_row(title.into(), query, CardVariant::Square, Some(at), on_open)
    }))
}
