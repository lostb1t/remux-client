use anyhow::{anyhow, Error, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use strum_macros::Display as EnumDisplay;
use strum_macros::EnumString;
use uuid::Uuid;

use crate::sdks;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Rating {
    pub source: RatingSource,
    /// Scale of 0 - 100.
    pub score: u32,
}

impl Rating {
    pub fn format_score(&self) -> String {
        match self.source {
            RatingSource::RottenTomatoes => self.score.to_string(),
            RatingSource::Audience => format!("{:.1}", self.score as f32 / 10.0),
        }
    }

    /// The icon of a provider; `None` for the audience score, which has a star.
    pub fn icon(&self) -> Option<nsh_core::element::ImageAsset> {
        use nsh_core::prelude::asset;
        match self.source {
            RatingSource::RottenTomatoes if self.score < 50 => Some(asset!("img/icon/rt-rotten.png")),
            RatingSource::RottenTomatoes => Some(asset!("img/icon/rt.svg")),
            RatingSource::Audience => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RatingSource {
    RottenTomatoes,
    /// What viewers score it: the average of the providers the server knows, or TMDb's when that
    /// is all it has.
    Audience,
}

#[derive(PartialEq, Eq, Clone, Hash, Debug, Default, EnumString, EnumDisplay)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum MediaType {
    #[default]
    #[strum(to_string = "Movie")]
    Movie,
    #[strum(serialize = "tv", to_string = "TV")]
    Series,
    #[strum(to_string = "season")]
    Season,
    #[strum(to_string = "episode")]
    Episode,
    /// A live TV channel: it is played, not opened.
    #[strum(to_string = "channel")]
    Channel,
    /// A programme of a live channel.
    #[strum(to_string = "program")]
    Program,
    /// A track of music: it is played.
    #[strum(to_string = "audio")]
    Audio,
    /// A record: the tracks of it, in order.
    #[strum(to_string = "album")]
    Album,
    #[strum(to_string = "artist")]
    MusicArtist,
    #[strum(to_string = "person")]
    Person,
    /// A list of titles or tracks that someone put together.
    #[strum(to_string = "playlist")]
    Playlist,
    /// A collection (box set): films, shows or other collections that belong together.
    #[strum(to_string = "collection")]
    Collection,
}

impl TryFrom<sdks::MediaType> for MediaType {
    type Error = Error;

    fn try_from(kind: sdks::MediaType) -> Result<Self, Self::Error> {
        match kind {
            sdks::MediaType::Movie => Ok(MediaType::Movie),
            sdks::MediaType::Series => Ok(MediaType::Series),
            sdks::MediaType::Season => Ok(MediaType::Season),
            sdks::MediaType::Episode => Ok(MediaType::Episode),
            sdks::MediaType::TvChannel | sdks::MediaType::LiveTvChannel => Ok(MediaType::Channel),
            sdks::MediaType::Program | sdks::MediaType::TvProgram | sdks::MediaType::LiveTvProgram => Ok(MediaType::Program),
            sdks::MediaType::Audio => Ok(MediaType::Audio),
            sdks::MediaType::MusicAlbum => Ok(MediaType::Album),
            sdks::MediaType::MusicArtist => Ok(MediaType::MusicArtist),
            sdks::MediaType::Person => Ok(MediaType::Person),
            sdks::MediaType::Playlist => Ok(MediaType::Playlist),
            sdks::MediaType::BoxSet => Ok(MediaType::Collection),
            _ => Err(anyhow!("Unsupported MediaType: {kind:?}")),
        }
    }
}

impl From<MediaType> for sdks::MediaType {
    fn from(value: MediaType) -> Self {
        match value {
            MediaType::Movie => sdks::MediaType::Movie,
            MediaType::Series => sdks::MediaType::Series,
            MediaType::Season => sdks::MediaType::Season,
            MediaType::Episode => sdks::MediaType::Episode,
            MediaType::Channel => sdks::MediaType::TvChannel,
            MediaType::Program => sdks::MediaType::Program,
            MediaType::Audio => sdks::MediaType::Audio,
            MediaType::Album => sdks::MediaType::MusicAlbum,
            MediaType::MusicArtist => sdks::MediaType::MusicArtist,
            MediaType::Person => sdks::MediaType::Person,
            MediaType::Playlist => sdks::MediaType::Playlist,
            MediaType::Collection => sdks::MediaType::BoxSet,
        }
    }
}

#[derive(PartialEq, Eq, Default, Hash, Clone, Debug)]
pub struct ExternalIds {
    pub imdb: Option<String>,
    pub tmdb: Option<u32>,
}

#[derive(Copy, Debug, Clone, Eq, PartialEq, Hash)]
pub enum MediaStreamType {
    Audio,
    Video,
    Subtitle,
    EmbeddedImage,
    Data,
    Lyric,
}

impl From<sdks::MediaStreamType> for MediaStreamType {
    fn from(t: sdks::MediaStreamType) -> Self {
        match t {
            sdks::MediaStreamType::Audio => MediaStreamType::Audio,
            sdks::MediaStreamType::Video => MediaStreamType::Video,
            sdks::MediaStreamType::Subtitle => MediaStreamType::Subtitle,
            sdks::MediaStreamType::EmbeddedImage => MediaStreamType::EmbeddedImage,
            sdks::MediaStreamType::Data => MediaStreamType::Data,
            sdks::MediaStreamType::Lyric => MediaStreamType::Lyric,
        }
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub struct MediaSource {
    pub id: Uuid,
    pub name: String,
    pub media_streams: Vec<MediaStream>,
}

impl MediaSource {
    /// What the server calls this version, falling back to what its video is.
    pub fn title(&self) -> String {
        if self.name.is_empty() {
            self.subtitle().unwrap_or_default()
        } else {
            self.name.clone()
        }
    }

    /// Whether this is Dolby Vision with an HDR10 (or similar) layer under it, which a screen or
    /// decoder without Dolby Vision plays as that: profiles 7 and 8. Profile 5 has no such
    /// layer, and shows in wrong colours without a Dolby Vision decoder.
    pub fn dolby_vision_with_fallback(&self) -> bool {
        self.media_streams
            .iter()
            .find(|s| s.type_ == MediaStreamType::Video)
            .and_then(|s| s.dv_profile)
            .is_some_and(|profile| profile == 7 || profile == 8)
    }

    /// What its video is, as the server describes it: "1080p H264 Sdr".
    pub fn subtitle(&self) -> Option<String> {
        self.media_streams
            .iter()
            .find(|s| s.type_ == MediaStreamType::Video)
            .and_then(|s| s.display_title.clone())
    }
}

impl From<sdks::MediaSourceInfo> for MediaSource {
    fn from(info: sdks::MediaSourceInfo) -> Self {
        MediaSource {
            id: info.id,
            name: info.name.unwrap_or_default(),
            media_streams: info
                .media_streams
                .into_iter()
                .filter_map(|s| MediaStream::try_from(s).ok())
                .collect(),
        }
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub struct MediaStream {
    pub index: i64,
    pub title: Option<String>,
    pub display_title: Option<String>,
    pub type_: MediaStreamType,
    pub bit_rate: Option<i64>,
    /// The Dolby Vision profile of a video, when it has one.
    pub dv_profile: Option<i64>,
}

impl TryFrom<sdks::MediaStream> for MediaStream {
    type Error = Error;

    fn try_from(stream: sdks::MediaStream) -> Result<Self, Self::Error> {
        Ok(MediaStream {
            index: stream.index,
            title: stream.title,
            display_title: stream.display_title,
            type_: stream.type_.into(),
            bit_rate: stream.bit_rate,
            dv_profile: stream.dv_profile,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Copy)]
pub enum ImageType {
    Poster,
    PosterTextless,
    Backdrop,
    Thumb,
    Logo,
}

#[derive(Default, Eq, PartialEq, Hash, Clone, Debug)]
pub struct Genre {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub struct UserData {
    pub playback_position_ticks: i64,
    pub play_count: i32,
    pub is_favorite: bool,
    pub is_watched: bool,
}

impl From<sdks::UserItemDataDto> for UserData {
    fn from(t: sdks::UserItemDataDto) -> Self {
        UserData {
            playback_position_ticks: t.playback_position_ticks,
            play_count: t.play_count,
            is_favorite: t.is_favorite,
            is_watched: t.played,
        }
    }
}

/// Which image types an item has. Presence is all that matters: the image itself is
/// fetched by item id.
#[derive(Default, Eq, PartialEq, Hash, Clone, Debug)]
pub struct Images {
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub thumb: Option<String>,
    pub logo: Option<String>,
    /// The item whose thumb and backdrop an episode or season shows when it has none of its own:
    /// its show.
    pub parent_thumb: Option<Uuid>,
    pub parent_backdrop: Option<Uuid>,
    /// The items whose poster an episode shows when it has none of its own: its season, and
    /// failing that its show.
    pub parent_poster: Option<Uuid>,
    pub series_poster: Option<Uuid>,
}

impl Images {
    pub fn tag(&self, image_type: ImageType) -> Option<&str> {
        match image_type {
            ImageType::Poster => self.poster.as_deref(),
            ImageType::Backdrop => self.backdrop.as_deref(),
            ImageType::Thumb => self.thumb.as_deref(),
            ImageType::Logo => self.logo.as_deref(),
            ImageType::PosterTextless => None,
        }
    }
}

#[derive(Copy, Debug, Clone, Eq, PartialEq, Hash)]
pub enum PersonKind {
    Actor,
    Director,
    Writer,
    Producer,
    Other,
}

impl From<Option<&str>> for PersonKind {
    fn from(kind: Option<&str>) -> Self {
        match kind {
            Some("Actor" | "GuestStar") => PersonKind::Actor,
            Some("Director") => PersonKind::Director,
            Some("Writer") => PersonKind::Writer,
            Some("Producer") => PersonKind::Producer,
            _ => PersonKind::Other,
        }
    }
}

/// Someone who worked on an item: a cast member or crew.
#[derive(Eq, Hash, PartialEq, Clone, Debug)]
pub struct Person {
    pub id: Uuid,
    pub name: String,
    /// The character, for an actor.
    pub role: Option<String>,
    pub kind: PersonKind,
    pub has_image: bool,
}

#[derive(Eq, Hash, PartialEq, Clone, Debug)]
pub struct Media {
    pub id: Uuid,
    pub title: String,
    pub media_type: MediaType,
    pub external_ids: ExternalIds,
    pub description: Option<String>,
    pub release_date: Option<DateTime<Utc>>,
    pub runtime_seconds: Option<i64>,
    pub images: Images,
    pub genres: Vec<String>,
    pub index_number: Option<i64>,
    pub user_data: Option<UserData>,
    pub official_rating: Option<String>,
    pub ratings: Vec<Rating>,
    pub media_sources: Vec<MediaSource>,
    pub tagline: Option<String>,
    /// For episodes: the show, the season and the season's number.
    pub series_id: Option<Uuid>,
    pub series_name: Option<String>,
    pub season_id: Option<Uuid>,
    pub season_number: Option<i64>,
    pub studios: Vec<String>,
    /// For a person: where they were born.
    pub production_locations: Vec<String>,
    /// Only filled when the item was fetched for its detail page.
    pub people: Vec<Person>,
    /// On the viewer's watchlist; only a Remux server says.
    pub watchlist: bool,
    /// For a track: who plays it, and the artist's page.
    pub artist: Option<String>,
    pub artist_id: Option<Uuid>,
    /// For a track: the album it is on. For an album: who made it (the album artist).
    pub album: Option<String>,
    pub album_id: Option<Uuid>,
    pub album_artist: Option<String>,
    pub album_artist_id: Option<Uuid>,
    /// For a programme: the channel, and when it is on.
    pub channel_id: Option<Uuid>,
    pub channel_name: Option<String>,
    /// Which entry of a playlist this is, when it is listed as one: what takes it out again.
    pub playlist_item_id: Option<String>,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
}

impl Media {
    pub fn people_of(&self, kind: PersonKind) -> impl Iterator<Item = &Person> {
        self.people.iter().filter(move |person| person.kind == kind)
    }

    /// The programme this is, for an item that is one.
    pub fn program(&self) -> Option<Program> {
        Some(Program {
            id: self.id,
            title: self.title.clone(),
            episode_title: None,
            overview: self.description.clone(),
            channel_id: self.channel_id?,
            channel_name: self.channel_name.clone().unwrap_or_default(),
            start: self.start?,
            end: self.end?,
        })
    }

    /// A live channel: there is no start, end or position to it.
    pub fn is_live(&self) -> bool {
        self.media_type == MediaType::Channel
    }

    pub fn is_series(&self) -> bool {
        self.media_type == MediaType::Series
    }

    pub fn is_favorite(&self) -> bool {
        self.user_data.as_ref().is_some_and(|d| d.is_favorite)
    }

    pub fn is_watched(&self) -> bool {
        self.user_data.as_ref().is_some_and(|d| d.is_watched)
    }

    pub fn is_watchlist(&self) -> bool {
        self.watchlist
    }

    pub fn formatted_runtime(&self) -> String {
        let duration = Duration::seconds(self.runtime_seconds.unwrap_or(0));
        let hours = duration.num_hours();
        let minutes = duration.num_minutes() % 60;

        match (hours, minutes) {
            (0, 0) => "0m".to_string(),
            (0, m) => format!("{m}m"),
            (h, 0) => format!("{h}h"),
            (h, m) => format!("{h}h {m}m"),
        }
    }

    /// Where to resume, in ms: the saved position, unless the item is all but finished.
    pub fn resume_ms(&self) -> f64 {
        let ticks = self.user_data.as_ref().map_or(0, |d| d.playback_position_ticks);
        let position = ticks as f64 / 10_000.0;
        let runtime = self.runtime_seconds.unwrap_or(0) as f64 * 1000.0;
        if runtime > 0.0 && position > runtime * 0.95 {
            0.0
        } else {
            position
        }
    }

    /// Playback progress in percent, when the item is partly watched.
    pub fn progress(&self) -> Option<u32> {
        let ticks = self.user_data.as_ref()?.playback_position_ticks;
        let runtime = self.runtime_seconds?;
        if ticks == 0 || runtime == 0 {
            return None;
        }
        Some(((ticks / 10_000_000 * 100) / runtime).min(100) as u32)
    }

    /// A textless poster from TMDB, if the item has a TMDB id and a read token is configured.
    pub async fn get_poster_textless(&self) -> Result<Option<String>> {
        let Some(tmdb_id) = self.external_ids.tmdb else {
            return Ok(None);
        };
        let kind = match self.media_type {
            MediaType::Movie => "movie",
            MediaType::Series => "tv",
            _ => return Ok(None),
        };
        crate::tmdb::textless_poster(kind, tmdb_id).await
    }
}

impl TryFrom<sdks::BaseItemDto> for Genre {
    type Error = Error;

    fn try_from(item: sdks::BaseItemDto) -> Result<Self, Self::Error> {
        Ok(Genre {
            id: item.id.to_string(),
            name: item.name.ok_or_else(|| anyhow!("Missing genre name"))?,
        })
    }
}

impl TryFrom<sdks::BaseItemDto> for Media {
    type Error = Error;

    fn try_from(item: sdks::BaseItemDto) -> Result<Self, Self::Error> {
        let tags = item.image_tags.unwrap_or_default();
        let provider_ids = item.provider_ids.unwrap_or_default();

        let mut ratings = Vec::new();
        if let Some(rating) = item.critic_rating {
            ratings.push(Rating {
                source: RatingSource::RottenTomatoes,
                score: rating as u32,
            });
        }
        if let Some(rating) = item.community_rating {
            ratings.push(Rating {
                source: RatingSource::Audience,
                score: (rating * 10.0) as u32,
            });
        }

        let media = Media {
            id: item.id,
            title: item.name.ok_or_else(|| anyhow!("Missing item name"))?,
            media_type: item.type_.try_into()?,
            external_ids: ExternalIds {
                tmdb: provider_ids.tmdb.and_then(|v| v.parse().ok()),
                imdb: provider_ids.imdb,
            },
            description: item.overview,
            release_date: item.premiere_date,
            runtime_seconds: item.run_time_ticks.map(|ticks| ticks / 10_000_000),
            images: Images {
                poster: tags.primary,
                backdrop: item.backdrop_image_tags.into_iter().next(),
                thumb: tags.thumb,
                logo: tags.logo,
                parent_thumb: item.parent_thumb_item_id.filter(|_| item.parent_thumb_image_tag.is_some()),
                parent_backdrop: item
                    .parent_backdrop_item_id
                    .filter(|_| item.parent_backdrop_image_tags.as_ref().is_some_and(|tags| !tags.is_empty())),
                parent_poster: item.parent_primary_image_item_id.filter(|_| item.parent_primary_image_tag.is_some()),
                series_poster: item.series_id.filter(|_| item.series_primary_image_tag.is_some()),
            },
            genres: item.genres,
            index_number: item.index_number,
            user_data: item.user_data.map(Into::into),
            official_rating: item.official_rating,
            ratings,
            media_sources: item.media_sources.unwrap_or_default().into_iter().map(Into::into).collect(),
            tagline: item.taglines.into_iter().next(),
            series_id: item.series_id,
            series_name: item.series_name,
            season_id: item.season_id,
            season_number: item.parent_index_number,
            studios: item.studios.into_iter().map(|studio| studio.name).collect(),
            production_locations: item.production_locations.unwrap_or_default(),
            watchlist: item.remux.as_ref().and_then(|remux| remux.watchlist).unwrap_or(false),
            artist: item.artists.first().cloned(),
            artist_id: item.artist_items.first().map(|artist| artist.id),
            album: item.album.clone().filter(|album| !album.is_empty()),
            album_id: item.album_id,
            album_artist: item.album_artist.clone().filter(|artist| !artist.is_empty()),
            album_artist_id: item.album_artists.first().map(|artist| artist.id),
            channel_id: item.channel_id,
            channel_name: item.channel_name.as_deref().map(channel_name),
            playlist_item_id: item.playlist_item_id.clone(),
            start: item.start_date.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&Utc)),
            end: item.end_date.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&Utc)),
            people: item
                .people
                .into_iter()
                .map(|person| Person {
                    id: person.id,
                    name: person.name,
                    role: person.role.filter(|role| !role.is_empty()),
                    kind: person.type_.as_deref().into(),
                    has_image: person.primary_image_tag.is_some(),
                })
                .collect(),
        };
        remember(&media);
        Ok(media)
    }
}

/// The items that lists loaded lately, by id. A title's page opens from one of them and shows what
/// it has (the picture, the story, the ratings) at once, while the page's own request, which also
/// brings the cast, is still on its way. Only for a first look: that request has the last word.
static RECENT: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<Uuid, Media>>> =
    std::sync::LazyLock::new(Default::default);

/// How many items are kept; when there are more, all are let go (the lists load them again).
const RECENT_LIMIT: usize = 750;

fn remember(media: &Media) {
    if let Ok(mut recent) = RECENT.lock() {
        if recent.len() >= RECENT_LIMIT {
            recent.clear();
        }
        recent.insert(media.id, media.clone());
    }
}

/// What a list loaded of this item lately, if it did.
pub fn recent(id: Uuid) -> Option<Media> {
    RECENT.lock().ok()?.get(&id).cloned()
}

/// A library of the server, such as Movies or Shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    pub id: Uuid,
    pub name: String,
    /// Whether it has a primary image, which for a library is a banner.
    pub has_image: bool,
    /// Live TV: the server always puts it last, so it can't be moved.
    pub live_tv: bool,
}

/// How a list on the Discover page is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sort {
    Popular,
    Trending,
    CommunityRating,
    CriticRating,
    Added,
    Released,
    Name,
    /// Not one to choose from (it is not in [`Sort::ALL`]): the order of the tracks of an album,
    /// by disc and then by number.
    TrackNumber,
    /// Not one to choose from either: what has been played the most, over all of the time.
    Plays,
    /// And not this one: whatever comes up, for a shuffle.
    Random,
}

impl Sort {
    pub const ALL: [Sort; 7] = [
        Sort::Popular,
        Sort::Trending,
        Sort::CommunityRating,
        Sort::CriticRating,
        Sort::Added,
        Sort::Released,
        Sort::Name,
    ];

    /// Only a Remux server has these: a Jellyfin server doesn't know them.
    pub fn remux_only(self) -> bool {
        matches!(self, Sort::Popular | Sort::Trending)
    }

    pub fn label(self) -> &'static str {
        match self {
            Sort::Popular => "Popular",
            Sort::Trending => "Trending",
            Sort::CommunityRating => "Community rating",
            Sort::CriticRating => "Critic rating",
            Sort::Added => "Recently added",
            Sort::Released => "Release date",
            Sort::Name => "Name",
            Sort::TrackNumber => "Track number",
            Sort::Plays => "Plays",
            Sort::Random => "Random",
        }
    }

    pub fn item_sort(self) -> sdks::ItemSortBy {
        match self {
            Sort::Popular => sdks::ItemSortBy::PopularityWeek,
            Sort::Trending => sdks::ItemSortBy::TrendingWeek,
            Sort::CommunityRating => sdks::ItemSortBy::CommunityRating,
            Sort::CriticRating => sdks::ItemSortBy::CriticRating,
            Sort::Added => sdks::ItemSortBy::DateCreated,
            Sort::Released => sdks::ItemSortBy::PremiereDate,
            Sort::Name => sdks::ItemSortBy::SortName,
            Sort::TrackNumber => sdks::ItemSortBy::IndexNumber,
            Sort::Plays => sdks::ItemSortBy::PopularityAllTime,
            Sort::Random => sdks::ItemSortBy::Random,
        }
    }

    /// What the server sorts by, in order: a track goes by its disc first.
    pub fn item_sorts(self) -> Vec<sdks::ItemSortBy> {
        match self {
            Sort::TrackNumber => vec![sdks::ItemSortBy::ParentIndexNumber, sdks::ItemSortBy::IndexNumber],
            sort => vec![sort.item_sort()],
        }
    }

    /// Which way it goes first: the most popular, the best, the newest; a name from A.
    pub fn descending_first(self) -> bool {
        !matches!(self, Sort::Name | Sort::TrackNumber | Sort::Random)
    }
}

/// Which "row" of the home screen a query belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CatalogId {
    /// What the hero shows: what is being watched, and popular things to fill up.
    Hero,
    /// The next episode of each show the viewer is watching.
    NextUp,
    Favorites,
    /// A collection (box set) on the server.
    Collection(Uuid),
}

/// A row on the home screen. The server decides which there are and in what order.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    pub id: CatalogId,
    pub title: String,
    pub card_variant: CardVariant,
    /// What the library is for, when the row is a library.
    pub library: Option<LibraryKind>,
}

/// The kind of a library: it decides whether its row is on the home screen for a chosen kind of
/// item, and what the row is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryKind {
    Movies,
    Shows,
    /// Films, shows, or loose episodes: whatever there is.
    Mixed,
}

impl Catalog {
    pub fn new(id: CatalogId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            card_variant: CardVariant::default(),
            library: None,
        }
    }
}


/// A programme of a live channel, as the guide has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub id: Uuid,
    pub title: String,
    pub episode_title: Option<String>,
    pub overview: Option<String>,
    pub channel_id: Uuid,
    pub channel_name: String,
    pub start: chrono::DateTime<chrono::Utc>,
    pub end: chrono::DateTime<chrono::Utc>,
}

impl TryFrom<sdks::BaseItemDto> for Program {
    type Error = Error;

    fn try_from(item: sdks::BaseItemDto) -> Result<Self, Self::Error> {
        let time = |text: Option<String>| {
            text.and_then(|text| chrono::DateTime::parse_from_rfc3339(&text).ok())
                .map(|time| time.with_timezone(&chrono::Utc))
        };
        Ok(Program {
            id: item.id,
            title: item.name.ok_or_else(|| anyhow!("Missing programme name"))?,
            episode_title: item.episode_title.filter(|title| !title.is_empty()),
            overview: item.overview.filter(|text| !text.is_empty()),
            channel_id: item.channel_id.ok_or_else(|| anyhow!("Missing channel"))?,
            channel_name: item.channel_name.map(|name| channel_name(&name)).unwrap_or_default(),
            start: time(item.start_date).ok_or_else(|| anyhow!("Missing start"))?,
            end: time(item.end_date).ok_or_else(|| anyhow!("Missing end"))?,
        })
    }
}

impl Program {
    /// On the air at `now`.
    pub fn airing(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.start <= now && now < self.end
    }

    /// How far it is, in percent, at `now`.
    pub fn progress(&self, now: chrono::DateTime<chrono::Utc>) -> f64 {
        let length = (self.end - self.start).num_seconds().max(1) as f64;
        ((now - self.start).num_seconds() as f64 / length * 100.0).clamp(0.0, 100.0)
    }

    /// The minutes that are left at `now`.
    pub fn minutes_left(&self, now: chrono::DateTime<chrono::Utc>) -> i64 {
        ((self.end - now).num_seconds() + 59) / 60
    }

    /// "08:00-09:00", in the time of the viewer.
    pub fn time_range(&self) -> String {
        use chrono::Local;
        format!(
            "{}-{}",
            self.start.with_timezone(&Local).format("%H:%M"),
            self.end.with_timezone(&Local).format("%H:%M")
        )
    }
}

/// What a channel is called, without what the lists it comes from add: the quality ("(1080p)"),
/// and the rest of a line of the list that was cut in the wrong place.
pub fn channel_name(name: &str) -> String {
    let name = match name.rsplit_once("\",") {
        Some((_, last)) if name.contains("group-title=") => last,
        _ => name,
    };
    // "(1080p)", wherever it is: "A1 TV (1080p) [Not 24/7]" is "A1 TV [Not 24/7]".
    let mut name = name.to_string();
    let mut from = 0;
    while let Some(open) = name[from..].find(" (").map(|at| at + from) {
        let rest = &name[open + 2..];
        match rest.find(')') {
            Some(close) if rest[..close].ends_with('p') && rest[..close - 1].chars().all(|c| c.is_ascii_digit()) && close > 1 => {
                name.replace_range(open..open + 2 + close + 1, "");
            }
            _ => from = open + 2,
        }
    }
    name
}

/// The kinds of programme the landing page of Live TV has a row for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProgramKind {
    Movies,
    Series,
    Sports,
    News,
    Kids,
}

impl ProgramKind {
    pub const ALL: [ProgramKind; 5] =
        [ProgramKind::Movies, ProgramKind::Series, ProgramKind::Sports, ProgramKind::News, ProgramKind::Kids];

    pub fn label(self) -> &'static str {
        match self {
            ProgramKind::Movies => "Movies",
            ProgramKind::Series => "Series",
            ProgramKind::Sports => "Sports",
            ProgramKind::News => "News",
            ProgramKind::Kids => "Kids",
        }
    }

    /// The name of the filter the server has for it.
    pub fn param(self) -> &'static str {
        match self {
            ProgramKind::Movies => "isMovie",
            ProgramKind::Series => "isSeries",
            ProgramKind::Sports => "isSports",
            ProgramKind::News => "isNews",
            ProgramKind::Kids => "isKids",
        }
    }
}

/// The shape of the cards of a row (Remux's `components::CardVariant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardVariant {
    #[default]
    Poster,
    Square,
    Landscape,
    Hero,
    Unstyled,
}
