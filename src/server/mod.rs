use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use bon::Builder;
use std::cell::RefCell;
use std::collections::HashMap;
use uuid::Uuid;

use crate::media::{Catalog, CatalogId, Media, MediaType};

mod jellyfin;
pub use jellyfin::{JellyfinServer, QuickConnectRequest};

/// What to fetch from the server. Doubles as the cache key.
#[derive(Builder, Debug, Hash, Clone, PartialEq, Eq)]
pub struct MediaQuery {
    #[builder(default = 25)]
    pub limit: u32,

    #[builder(default = 0)]
    pub offset: u32,

    #[builder(default = vec![MediaType::Movie, MediaType::Series])]
    pub types: Vec<MediaType>,

    pub search_query: Option<String>,

    /// Only children of this item (seasons of a series, episodes of a season).
    pub parent: Option<Uuid>,

    pub catalog: Option<CatalogId>,

    /// Only what is in this playlist, in its order.
    pub playlist: Option<Uuid>,

    /// Only what this person is in, or what this artist plays.
    pub person: Option<Uuid>,
    pub artist: Option<Uuid>,
    /// Only the albums that this artist made (not the ones they are on).
    pub album_artist: Option<Uuid>,

    #[builder(default)]
    pub genres: Vec<String>,

    /// Leave out items that have been watched.
    #[builder(default = false)]
    pub hide_watched: bool,

    /// What was added to the library lately, and has not been watched: what Jellyfin shows on its
    /// home page, from `/Items/Latest`. The server gives it in one go, not in pages.
    #[builder(default = false)]
    pub latest: bool,

    /// The order, when it is not the server's. Ignored for a search, which goes by how well
    /// things match.
    pub sort: Option<crate::media::Sort>,
    #[builder(default = false)]
    pub descending: bool,

    /// Also what is in the folders and collections below `parent`. A collection is asked for
    /// without: its own items are what it has, not those of the collections in it.
    #[builder(default = true)]
    pub recursive: bool,
}

impl MediaQuery {
    /// Stable identity for use as an element `key`.
    pub fn key(&self) -> String {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

/// Bumped whenever user data (favorite, watched) changes, so cached lists are refetched.
static CACHE_EPOCH: AtomicU32 = AtomicU32::new(0);

pub fn invalidate_cache() {
    CACHE_EPOCH.fetch_add(1, Ordering::Relaxed);
}

/// Changes whenever user data does, so what is shown can tell it is out of date.
pub fn cache_epoch() -> u32 {
    CACHE_EPOCH.load(Ordering::Relaxed)
}

/// What the server answered, kept for five minutes (Remux's `cached` `TimedCache`s; NSH's clock,
/// which a browser has too, unlike std's `Instant`). On the UI thread, where every request is made.
struct Kept<K, V> {
    entries: RefCell<HashMap<K, (f64, V)>>,
}

impl<K: Hash + Eq, V: Clone> Kept<K, V> {
    const LIFE_MS: f64 = 300_000.0;

    fn new() -> Self {
        Self { entries: RefCell::new(HashMap::new()) }
    }

    fn get(&self, key: &K) -> Option<V> {
        let now = nsh_core::time::unix_ms();
        let mut entries = self.entries.borrow_mut();
        match entries.get(key) {
            Some((at, value)) if now - at < Self::LIFE_MS => Some(value.clone()),
            Some(_) => {
                entries.remove(key);
                None
            }
            None => None,
        }
    }

    fn put(&self, key: K, value: V) {
        let now = nsh_core::time::unix_ms();
        let mut entries = self.entries.borrow_mut();
        // The old ones go now and then, so the map does not grow for good.
        if entries.len() > 500 {
            entries.retain(|_, (at, _)| now - *at < Self::LIFE_MS);
        }
        entries.insert(key, (now, value));
    }
}

thread_local! {
    static VIEWS: Kept<(u32, Uuid), Vec<crate::sdks::BaseItemDto>> = Kept::new();
    static LIVE_TV: Kept<Uuid, bool> = Kept::new();
    static MUSIC: Kept<Uuid, bool> = Kept::new();
    static MEDIA: Kept<(u32, Uuid, MediaQuery), Vec<Media>> = Kept::new();
}

/// The user's views, from which the libraries and the rows of the home screen are made: asked
/// once for both.
pub async fn get_user_views_cached(
    server: Arc<JellyfinServer>,
) -> Result<Vec<crate::sdks::BaseItemDto>, anyhow::Error> {
    let key = (cache_epoch(), server.user_id());
    if let Some(views) = VIEWS.with(|kept| kept.get(&key)) {
        return Ok(views);
    }
    let views = server.user_views().await?;
    VIEWS.with(|kept| kept.put(key, views.clone()));
    Ok(views)
}

pub async fn get_catalogs_cached(
    server: Arc<JellyfinServer>,
) -> Result<Vec<Catalog>, anyhow::Error> {
    Ok(JellyfinServer::catalogs_of(get_user_views_cached(server).await?))
}

pub async fn get_libraries_cached(
    server: Arc<JellyfinServer>,
) -> Result<Vec<crate::media::Library>, anyhow::Error> {
    Ok(JellyfinServer::libraries_of(get_user_views_cached(server).await?))
}

/// Whether the user has Live TV: the server lists it among their views when there are channels.
pub async fn get_live_tv_cached(server: Arc<JellyfinServer>) -> Result<bool, anyhow::Error> {
    let key = server.user_id();
    if let Some(live) = LIVE_TV.with(|kept| kept.get(&key)) {
        return Ok(live);
    }
    let live = server.has_live_tv().await?;
    LIVE_TV.with(|kept| kept.put(key, live));
    Ok(live)
}

/// Whether the server has music for the user, which is what shows the Music chips.
pub async fn get_music_cached(server: Arc<JellyfinServer>) -> Result<bool, anyhow::Error> {
    let key = server.user_id();
    if let Some(music) = MUSIC.with(|kept| kept.get(&key)) {
        return Ok(music);
    }
    let music = server.has_music().await?;
    MUSIC.with(|kept| kept.put(key, music));
    Ok(music)
}

pub async fn get_media_cached(
    server: Arc<JellyfinServer>,
    query: &MediaQuery,
) -> Result<Vec<Media>, anyhow::Error> {
    let key = (cache_epoch(), server.user_id(), query.clone());
    if let Some(media) = MEDIA.with(|kept| kept.get(&key)) {
        return Ok(media);
    }
    let media = server.media(query).await?;
    MEDIA.with(|kept| kept.put(key, media.clone()));
    Ok(media)
}
