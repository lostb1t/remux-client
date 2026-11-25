//! Where the user is: one of the app's places (Home, Media, Discover, Live TV), each with a stack of
//! pages opened over it (Remux's `Route` and its `SubPane`s). Kept in a global, so anything can
//! open a page; a stack is pushed by putting a route in and popped by taking it out.

use nsh_core::prelude::*;
use uuid::Uuid;

use crate::media::MediaType;

/// A top-level place of the app: the tab bar on a phone, the menu on a TV.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Place {
    #[default]
    Home,
    Media,
    Discover,
    LiveTv,
}

impl Place {
    pub const ALL: [Place; 4] = [Place::Home, Place::Media, Place::Discover, Place::LiveTv];

    pub fn label(self) -> &'static str {
        match self {
            Place::Home => "Home",
            Place::Media => "Media",
            Place::Discover => "Discover",
            Place::LiveTv => "Live TV",
        }
    }

    pub fn index(self) -> usize {
        Place::ALL.iter().position(|place| *place == self).unwrap_or(0)
    }
}

/// A page opened over a place (Remux's routes, less the places themselves).
#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    /// A title: a film, a show, a season, an episode.
    MediaDetail { media_type: MediaType, id: Uuid },
    Person { id: Uuid, name: String },
    Artist { id: Uuid, name: String },
    Album { id: Uuid, name: String },
    Playlists,
    Playlist { id: Uuid, name: String },
    Collection { id: Uuid, name: String },
    LiveChannels,
    LiveGuide,
    Favorites,
    Library { id: Uuid, name: String },
    /// Every component of the `ui` layer (Design Guides): from the avatar, until Settings is ported.
    Gallery,
}

impl Route {
    /// What identifies its page in the stack: one page per route.
    pub fn key(&self) -> String {
        match self {
            Route::MediaDetail { id, .. } => format!("media/{id}"),
            Route::Person { id, .. } => format!("person/{id}"),
            Route::Artist { id, .. } => format!("artist/{id}"),
            Route::Album { id, .. } => format!("album/{id}"),
            Route::Playlists => "playlists".into(),
            Route::Playlist { id, .. } => format!("playlist/{id}"),
            Route::Collection { id, .. } => format!("collection/{id}"),
            Route::LiveChannels => "live/channels".into(),
            Route::LiveGuide => "live/guide".into(),
            Route::Favorites => "favorites".into(),
            Route::Library { id, .. } => format!("library/{id}"),
            Route::Gallery => "gallery".into(),
        }
    }

    /// What the page is called, for its header and for screen readers.
    pub fn title(&self) -> String {
        match self {
            Route::MediaDetail { media_type, .. } => media_type.to_string(),
            Route::Person { name, .. }
            | Route::Artist { name, .. }
            | Route::Album { name, .. }
            | Route::Playlist { name, .. }
            | Route::Collection { name, .. }
            | Route::Library { name, .. } => name.clone(),
            Route::Playlists => "Playlists".into(),
            Route::LiveChannels => "Channels".into(),
            Route::LiveGuide => "Guide".into(),
            Route::Favorites => "Favorites".into(),
            Route::Gallery => "Components".into(),
        }
    }
}

/// The place shown, and the pages open over each place, bottom to top.
#[derive(Clone, Default, PartialEq)]
struct Nav {
    place: Place,
    stacks: [Vec<Route>; 4],
}

fn nav() -> Signal<Nav> {
    global::<Nav>()
}

/// The place shown (tracked where it is read).
pub fn place() -> Place {
    nav().with(|nav| nav.place)
}

/// The pages open over `place` (tracked where it is read).
pub fn routes(place: Place) -> Vec<Route> {
    nav().with(|nav| nav.stacks[place.index()].clone())
}

/// Goes to `place`; going to the place that is shown goes back to its first page, as a tab bar
/// does.
pub fn go(place: Place) {
    nav().update(|nav| {
        if nav.place == place {
            nav.stacks[place.index()].clear();
        }
        nav.place = place;
    });
}

/// Opens the page of `route` over the place shown. A route already on top opens nothing more (a
/// second tap).
pub fn push(route: Route) {
    let place = place_now();
    let pushed = nav().with_untracked(|nav| nav.stacks[place.index()].last() != Some(&route));
    if !pushed {
        return;
    }
    nav().update(|nav| nav.stacks[place.index()].push(route));
    crate::tv_focus::opened(place, depth_now(place));
}

/// The place shown, not tracked (for listeners).
pub fn place_untracked() -> Place {
    place_now()
}

fn place_now() -> Place {
    nav().with_untracked(|nav| nav.place)
}

fn depth_now(place: Place) -> usize {
    nav().with_untracked(|nav| nav.stacks[place.index()].len())
}

/// Opens the page of `media`, the one of its kind: a title's, a collection's, an album's, an
/// artist's or a person's, a playlist's.
pub fn open_media(media: crate::media::Media) {
    let (id, name) = (media.id, media.title.clone());
    // A song plays (a card of one, on Home or in a search).
    if media.media_type == MediaType::Audio {
        crate::music::play(vec![media], 0);
        return;
    }
    push(match media.media_type {
        MediaType::Collection => Route::Collection { id, name },
        MediaType::Album => Route::Album { id, name },
        MediaType::MusicArtist => Route::Artist { id, name },
        MediaType::Person => Route::Person { id, name },
        MediaType::Playlist => Route::Playlist { id, name },
        media_type => Route::MediaDetail { media_type, id },
    });
}

/// Closes the page on top of the place shown.
pub fn pop() {
    let place = place_now();
    nav().update(|nav| {
        nav.stacks[place.index()].pop();
    });
    crate::tv_focus::closed(place, depth_now(place));
}

/// How many pages are open over `place` (tracked where it is read).
pub fn depth(place: Place) -> usize {
    nav().with(|nav| nav.stacks[place.index()].len())
}

/// Whether a page is open over the place shown (tracked where it is read).
pub fn can_go_back() -> bool {
    nav().with(|nav| !nav.stacks[nav.place.index()].is_empty())
}

/// Leaves `depth` pages over `place`: the user went back natively (the back button, a swipe).
pub fn truncate(place: Place, depth: usize) {
    if depth_now(place) <= depth {
        return;
    }
    nav().update(|nav| nav.stacks[place.index()].truncate(depth));
    crate::tv_focus::closed(place, depth);
}
