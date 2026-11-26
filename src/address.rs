//! An address for each page on the web (`#/discover/title/movie/<id>`): the browser's back and
//! forward go through the app's pages, a reload opens the page that was open, and an address can
//! be shared. Remux's router did this with paths; here it is the address's fragment, so the page
//! needs nothing of the server that hosts it (and works under `/remux/` as at the root).
//!
//! The app's place and pages (`nav`) are what counts; the address follows them. A page opened is
//! an entry of the browser's history of its own, another place replaces the entry, and going back
//! in the app goes back in the browser's history, so that it stays as the pages are. The
//! browser's back and forward (and an address typed) come back as the address to show.

#[cfg(target_arch = "wasm32")]
use nsh_core::element::{app_url, next_app_url_change, set_app_url_with};
#[cfg(target_arch = "wasm32")]
use nsh_core::prelude::*;
#[cfg(target_arch = "wasm32")]
use nsh_core::url_scheme::HistoryBehavior;

use crate::media::MediaType;
use crate::nav::{self, Place, Route};

#[cfg(target_arch = "wasm32")]
/// The addresses of the browser's history that are the app's, and the one shown: the app's own
/// record of the entries it made, to tell a step back from a page opened.
#[derive(Default)]
struct History {
    entries: Vec<String>,
    at: usize,
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static HISTORY: std::cell::RefCell<History> = std::cell::RefCell::default();
}

#[cfg(target_arch = "wasm32")]
/// Starts it: once, from the shell. The pages of the address the page was opened at, then the
/// address following the pages, and the browser's moves followed.
pub fn start() {
    spawn(async {
        let opened = app_url().await.unwrap_or_default();
        let (place, routes) = parse(&opened);
        let address = format(place, &routes);
        HISTORY.with(|history| {
            let mut history = history.borrow_mut();
            history.entries = vec![address.clone()];
            history.at = 0;
        });
        let _ = set_app_url_with(address, HistoryBehavior::Replace);
        if place != Place::Home || !routes.is_empty() {
            nav::show(place, routes);
        }
        // The address follows the pages from now on.
        effect(|| {
            let place = nav::place();
            let routes = nav::routes(place);
            untracked(|| follow(format(place, &routes)));
        });
        // The browser's back and forward, and an address typed.
        loop {
            let change = next_app_url_change().await;
            let address = change.url;
            let fragment = fragment_of(&address);
            HISTORY.with(|history| {
                let mut history = history.borrow_mut();
                match history.entries.iter().position(|entry| *entry == fragment) {
                    Some(at) => history.at = at,
                    None => {
                        let at = history.at + 1;
                        history.entries.truncate(at);
                        history.entries.push(fragment.clone());
                        history.at = at;
                    }
                }
            });
            let (place, routes) = parse(&fragment);
            nav::show(place, routes);
        }
    });
}

#[cfg(target_arch = "wasm32")]
/// The address of the pages shown now: a page opened over the ones before is an entry of its
/// own; one the app went back to is a step back in the browser's history; anything else (another
/// place) takes the place of the address shown.
fn follow(address: String) {
    enum Step {
        Same,
        Back(i32),
        Push,
        Replace,
    }
    let step = HISTORY.with(|history| {
        let history = history.borrow();
        let now = history.entries.get(history.at).cloned().unwrap_or_default();
        if now == address {
            return Step::Same;
        }
        if let Some(at) = history.entries[..history.at].iter().rposition(|entry| *entry == address) {
            return Step::Back(at as i32 - history.at as i32);
        }
        // Deeper in the same place: a page opened.
        let (place_now, pages_now) = parse(&now);
        let (place, pages) = parse(&address);
        if place == place_now && pages.len() > pages_now.len() && pages.starts_with(&pages_now) {
            Step::Push
        } else {
            Step::Replace
        }
    });
    match step {
        Step::Same => {}
        // The browser goes back, and says so (`next_app_url_change`), which sets where it is.
        Step::Back(steps) => go_back(steps),
        Step::Push => {
            HISTORY.with(|history| {
                let mut history = history.borrow_mut();
                let at = history.at + 1;
                history.entries.truncate(at);
                history.entries.push(address.clone());
                history.at = at;
            });
            let _ = set_app_url_with(address, HistoryBehavior::Push);
        }
        Step::Replace => {
            HISTORY.with(|history| {
                let mut history = history.borrow_mut();
                let at = history.at;
                if let Some(entry) = history.entries.get_mut(at) {
                    *entry = address.clone();
                }
            });
            let _ = set_app_url_with(address, HistoryBehavior::Replace);
        }
    }
}

#[cfg(target_arch = "wasm32")]
/// Back `steps` (negative) in the browser's history. NSH has no call for it, so the browser's own
/// (`history.go`), through web-sys.
fn go_back(steps: i32) {
    if let Some(history) = web_sys::window().and_then(|window| window.history().ok()) {
        let _ = history.go_with_delta(steps);
    }
}

/// The fragment of an address (`#/…`), or "#/" for none.
fn fragment_of(address: &str) -> String {
    match address.find('#') {
        Some(at) => address[at..].to_owned(),
        None => "#/".to_owned(),
    }
}

/// `#/{place}/{page}/{page}…`, a page as `title/{type}/{id}`, `person/{id}/{name}` and so on.
pub fn format(place: Place, routes: &[Route]) -> String {
    let mut parts: Vec<String> = Vec::new();
    match place {
        Place::Home => {}
        Place::Media => parts.push("media".into()),
        Place::Discover => parts.push("discover".into()),
        Place::LiveTv => parts.push("live".into()),
    }
    if place == Place::Home && !routes.is_empty() {
        parts.push("home".into());
    }
    for route in routes {
        let named = |kind: &str, id: &uuid::Uuid, name: &str| format!("{kind}/{}/{}", id.simple(), encode(name));
        parts.push(match route {
            Route::MediaDetail { media_type, id } => format!("title/{}/{}", encode(&media_type.to_string().to_lowercase()), id.simple()),
            Route::Person { id, name } => named("person", id, name),
            Route::Artist { id, name } => named("artist", id, name),
            Route::Album { id, name } => named("album", id, name),
            Route::Playlist { id, name } => named("playlist", id, name),
            Route::Collection { id, name } => named("collection", id, name),
            Route::Library { id, name } => named("library", id, name),
            Route::Playlists => "playlists".into(),
            Route::LiveChannels => "channels".into(),
            Route::LiveGuide => "guide".into(),
            Route::Favorites => "favorites".into(),
        });
    }
    format!("#/{}", parts.join("/"))
}

/// The place and pages of an address; what it does not know is left out (an old or broken link
/// opens what it can, else Home).
pub fn parse(address: &str) -> (Place, Vec<Route>) {
    let fragment = fragment_of(address);
    let mut parts = fragment.trim_start_matches('#').split('/').filter(|part| !part.is_empty()).peekable();
    let place = match parts.peek().copied() {
        Some("media") => Place::Media,
        Some("discover") => Place::Discover,
        Some("live") => Place::LiveTv,
        Some("home") => Place::Home,
        _ => return (Place::Home, routes(parts)),
    };
    parts.next();
    (place, routes(parts))
}

fn routes<'a>(mut parts: impl Iterator<Item = &'a str>) -> Vec<Route> {
    let mut routes = Vec::new();
    while let Some(kind) = parts.next() {
        let id = |part: Option<&str>| part.and_then(|part| uuid::Uuid::parse_str(part).ok());
        let route = match kind {
            "title" => {
                let media_type = parts.next().and_then(|part| decode(part).parse::<MediaType>().ok());
                match (media_type, id(parts.next())) {
                    (Some(media_type), Some(id)) => Some(Route::MediaDetail { media_type, id }),
                    _ => None,
                }
            }
            "person" | "artist" | "album" | "playlist" | "collection" | "library" => {
                let (id, name) = (id(parts.next()), parts.next().map(decode).unwrap_or_default());
                id.map(|id| match kind {
                    "person" => Route::Person { id, name },
                    "artist" => Route::Artist { id, name },
                    "album" => Route::Album { id, name },
                    "playlist" => Route::Playlist { id, name },
                    "collection" => Route::Collection { id, name },
                    _ => Route::Library { id, name },
                })
            }
            "playlists" => Some(Route::Playlists),
            "channels" => Some(Route::LiveChannels),
            "guide" => Some(Route::LiveGuide),
            "favorites" => Some(Route::Favorites),
            _ => None,
        };
        match route {
            Some(route) => routes.push(route),
            None => break,
        }
    }
    routes
}

/// A name in an address: what is not a letter, digit, `-`, `_` or `.` as `%XX`.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&text[at + 1..at + 3], 16) {
                out.push(byte);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let id = uuid::Uuid::new_v4();
        let routes = vec![
            Route::Library { id, name: "Films & Series".into() },
            Route::MediaDetail { media_type: MediaType::Movie, id },
        ];
        let address = format(Place::Media, &routes);
        assert_eq!(parse(&address), (Place::Media, routes));
        assert_eq!(parse("#/"), (Place::Home, vec![]));
        assert_eq!(parse("https://x/remux/#/live/channels"), (Place::LiveTv, vec![Route::LiveChannels]));
        let home = vec![Route::Favorites];
        assert_eq!(parse(&format(Place::Home, &home)), (Place::Home, home));
    }
}
