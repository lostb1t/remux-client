use anyhow::{anyhow, Result};
use futures::stream::{FuturesOrdered, StreamExt};
use futures::FutureExt;
use derive_more::with_trait::Debug;
use log::debug;
use remux_sdks::Endpoint;
use uuid::Uuid;

use super::{invalidate_cache, MediaQuery};
use crate::session::Stored;
use crate::media::{self, Catalog, CatalogId, Media, Person};
use crate::sdks::{
    self, GetItems, GetItemsQuery, ImageQuery, ItemFields, ItemFilter, ItemSortBy, JellyfinAuth,
    SortOrder,
};

/// A sign-in with Quick Connect that waits for a device that is signed in to approve its code.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickConnectRequest {
    host: String,
    secret: String,
    /// The six digits to enter on the other device.
    pub code: String,
}

/// A logged in session on a Remux (or Jellyfin) server.
#[derive(Clone, Debug)]
pub struct JellyfinServer {
    pub username: String,
    /// The server's own id and name, as the session is kept with them.
    server_id: String,
    server_name: String,
    user_id: Uuid,
    #[debug(skip)]
    token: String,
    base: url::Url,
    #[debug(skip)]
    client: sdks::Api,
    /// Whether the server is a Remux one, asked of it once: it has more than a Jellyfin server.
    remux: std::sync::Arc<std::sync::OnceLock<bool>>,
}

impl PartialEq for JellyfinServer {
    fn eq(&self, other: &Self) -> bool {
        self.base == other.base && self.user_id == other.user_id
    }
}

fn client(host: &str, token: Option<&str>) -> Result<sdks::Api> {
    let app = crate::app_host::app_host();
    client_as(host, token, &app.device_name, &app.device_id)
}

/// A client that tells the server it is another device. The server goes by the token, not by
/// the id in the header, so this goes with a token that was made for that device.
fn client_as(
    host: &str,
    token: Option<&str>,
    device: &str,
    device_id: &str,
) -> Result<sdks::Api> {
    let auth = JellyfinAuth {
        client: "Remux".to_string(),
        device: device.to_string(),
        device_id: device_id.to_string(),
        version: crate::app_host::app_host().remux_version.clone(),
        token: token.map(str::to_string),
    };
    Ok(sdks::Api::new(host, auth)?)
}

/// Text for in a header: visible ASCII, without the quotes the header is made of.
fn header_text(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '?' })
        .map(|c| if c == '"' { '\'' } else { c })
        .collect()
}

/// Whether the request never got an answer from the server (as opposed to being refused).
fn is_unreachable(err: &anyhow::Error) -> bool {
    matches!(err.downcast_ref::<sdks::ApiError>(), Some(sdks::ApiError::Unreachable(_)))
}

impl JellyfinServer {
    /// Logs in. A host without a scheme is tried over https first, then over http when the
    /// server could not be reached; a wrong password or other server error is final.
    pub async fn from_credentials(host: &str, username: &str, password: &str) -> Result<Self> {
        let host = host.trim();
        if host.contains("://") {
            return Self::log_in(host, username, password).await;
        }

        match Self::log_in(&format!("https://{host}"), username, password).await {
            Err(err) if is_unreachable(&err) => {
                debug!("{host} is not reachable over https, trying http");
                Self::log_in(&format!("http://{host}"), username, password).await
            }
            result => result,
        }
    }

    async fn log_in(host: &str, username: &str, password: &str) -> Result<Self> {
        debug!("Connecting to {host}");
        let res = client(host, None)?
            .execute(sdks::AuthenticateUserByName {
                username: Some(username.to_string()),
                pw: Some(password.to_string()),
            })
            .await?;
        Self::from_authentication(host, res, username).await
    }

    /// The session the server made: its token, the user, and what the server calls itself.
    async fn from_authentication(host: &str, res: sdks::AuthenticationResult, username: &str) -> Result<Self> {
        let token = res
            .access_token
            .ok_or_else(|| anyhow!("Missing access token"))?;
        let user = res.user.ok_or_else(|| anyhow!("Missing user"))?;
        // What the server calls itself, for the session that is kept.
        let name = sdks::public_info(host)
            .await
            .and_then(|info| info["ServerName"].as_str().map(str::to_string))
        .unwrap_or_else(|| host.to_string());

        let username = if username.is_empty() { user.name.as_str() } else { username };
        Self::new(host, username, token, user.id, res.server_id, name)
    }

    /// Asks the server for a code, which a device that is signed in approves (in its settings).
    /// A host without a scheme is tried over https first, then over http, as for a login.
    pub async fn start_quick_connect(host: &str) -> Result<QuickConnectRequest> {
        let host = host.trim();
        let host = if host.contains("://") {
            host.to_string()
        } else if sdks::public_info(&format!("https://{host}")).await.is_some() {
            format!("https://{host}")
        } else {
            format!("http://{host}")
        };
        let client = client(&host, None)?;
        if !client.execute(sdks::QuickConnectEnabled).await.unwrap_or(false) {
            return Err(anyhow!("Quick Connect is not turned on for this server."));
        }
        let started = client.execute(sdks::InitiateQuickConnect).await?;
        Ok(QuickConnectRequest { host, secret: started.secret, code: started.code })
    }

    /// The session, once the code of `request` has been approved; `None` while it has not.
    pub async fn finish_quick_connect(request: &QuickConnectRequest) -> Result<Option<Self>> {
        let client = client(&request.host, None)?;
        let state = client.execute(sdks::GetQuickConnect { secret: request.secret.clone() }).await?;
        if !state.authenticated {
            return Ok(None);
        }
        let res = client.execute(sdks::SignInWithQuickConnect { secret: request.secret.clone() }).await?;
        Self::from_authentication(&request.host, res, "").await.map(Some)
    }

    pub fn from_stored(stored: Stored) -> Result<Self> {
        Self::new(&stored.address, &stored.username, stored.token, stored.user_id, stored.server_id, stored.name)
    }

    fn new(host: &str, username: &str, token: String, user_id: Uuid, server_id: String, server_name: String) -> Result<Self> {
        Ok(Self {
            username: username.to_string(),
            server_id,
            server_name,
            user_id,
            base: url::Url::parse(&format!("{}/", host.trim_end_matches('/')))?,
            client: client(host, Some(&token))?,
            token,
            remux: Default::default(),
        })
    }

    /// Signs in to an account that was kept, if its token is still good. `None` when the server
    /// no longer knows it: the account is taken out of the list then.
    pub async fn resume(stored: Stored) -> Result<Option<Self>> {
        let server = Self::from_stored(stored)?;
        match server.current_user().await? {
            Some(name) => Ok(Some(server.with_username(name))),
            None => {
                crate::session::forget(&server.to_stored());
                Ok(None)
            }
        }
    }

    /// Takes a kept account away: its token is ended at the server (if that can be reached) and
    /// it is not offered any more.
    pub async fn remove_account(stored: Stored) {
        if let Ok(server) = Self::from_stored(stored.clone()) {
            server.log_out().await;
        }
        crate::session::forget(&stored);
    }

    pub fn to_stored(&self) -> Stored {
        Stored {
            server_id: self.server_id.clone(),
            name: self.server_name.clone(),
            address: self.address(),
            token: self.token.clone(),
            user_id: self.user_id,
            username: self.username.clone(),
        }
    }

    /// Where the server is.
    pub fn address(&self) -> String {
        self.base.as_str().trim_end_matches('/').to_string()
    }

    /// The same session, now that the name of the user is known.
    pub fn with_username(&self, username: String) -> Self {
        Self { username, ..self.clone() }
    }

    /// Asks the server who the token is of: the name of the user, or that the token is not good
    /// any more (`None`), as opposed to the server not answering (an error).
    pub async fn current_user(&self) -> Result<Option<String>> {
        match self.client.execute(sdks::GetCurrentUser).await {
            Ok(user) => Ok(Some(user.name)),
            Err(sdks::ApiError::Unauthorized) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Whether the user is an administrator of the server.
    pub async fn is_admin(&self) -> bool {
        self.client
            .execute(sdks::GetCurrentUser)
            .await
            .map(|user| user.policy.is_administrator)
            .unwrap_or(false)
    }

    pub fn user_id(&self) -> Uuid {
        self.user_id
    }

    /// Whether this is a Remux server and not a plain Jellyfin one: its public info (which needs
    /// no login) names a Remux version. Asked once. A server that can't be asked counts as
    /// Jellyfin, which is the safe side: it is not shown what only a Remux server has.
    pub async fn is_remux(&self) -> bool {
        if let Some(known) = self.remux.get() {
            return *known;
        }
        let answer = async {
            let info = sdks::public_info(self.base.as_str()).await?;
            Some(info.get("RemuxVersion").is_some())
        }
        .await;
        match answer {
            Some(remux) => {
                let _ = self.remux.set(remux);
                remux
            }
            None => false,
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Tells the server what this device can play, ahead of the first playback: it keeps the
    /// profile on the device, to sort the versions of a title by.
    pub async fn report_device_profile(&self, device_profile: crate::sdks::DeviceProfile) -> Result<()> {
        self.client
            .execute(crate::sdks::PostFullCapabilities { device_profile })
            .await?;
        Ok(())
    }

    /// The server as another device of this user. The server keeps what a device plays with
    /// (the device profile, by which it sorts the versions of a title) on the device, and goes
    /// by the token to know which it is: so a Chromecast gets a session of its own, which does not
    /// change what this device is shown. `saved` is the token of an earlier one.
    ///
    /// The session is made with Quick Connect, which this session approves itself, so the
    /// viewer has nothing to do. A server with Quick Connect off is an error here.
    pub async fn session_for_device(
        &self,
        device_id: &str,
        device_name: &str,
        saved: Option<String>,
    ) -> Result<Self> {
        let host = self.base.as_str().trim_end_matches('/').to_string();
        let (device_id, device_name) = (header_text(device_id), header_text(device_name));
        let with_token = |token: String| -> Result<Self> {
            Ok(Self {
                username: self.username.clone(),
                server_id: self.server_id.clone(),
                server_name: self.server_name.clone(),
                user_id: self.user_id,
                base: self.base.clone(),
                client: client_as(&host, Some(&token), &device_name, &device_id)?,
                token,
                // The same server, so the same answer.
                remux: self.remux.clone(),
            })
        };

        if let Some(token) = saved {
            let session = with_token(token)?;
            let query = GetItemsQuery { limit: Some(1), ..session.items_query() };
            if session.client.execute(GetItems(query)).await.is_ok() {
                return Ok(session);
            }
            debug!("the saved session of {device_name} is no longer good");
        }

        // A session of that device: Quick Connect, started as it, approved by this session.
        let device = client_as(&host, None, &device_name, &device_id)?;
        let initiated = device.execute(sdks::InitiateQuickConnect).await?;
        self.authorize_quick_connect(&initiated.code).await?;
        let signed_in = device.execute(sdks::SignInWithQuickConnect { secret: initiated.secret }).await?;
        let token = signed_in
            .access_token
            .ok_or_else(|| anyhow!("Quick Connect gave no token"))?;
        with_token(token.to_string())
    }

    /// URL of an image of `media`, or `None` when it has none of that type.
    pub fn image_url(&self, media: &Media, image_type: media::ImageType) -> Option<String> {
        // PERFORMANCE: no larger than it is shown (with the pixels of a phone's screen, ~3 per
        // point, and a 4K TV for the backdrop): an original is decoded, and kept, in full.
        let max_width = match image_type {
            media::ImageType::Poster => 500,
            media::ImageType::Thumb | media::ImageType::Logo => 800,
            _ => 1920,
        };
        self.image_url_at(media, image_type, max_width)
    }

    /// As [`Self::image_url`], at most `max_width` pixels wide: for a picture that is shown larger
    /// than its kind usually is (a poster as the whole hero of a phone).
    pub fn image_url_at(&self, media: &Media, image_type: media::ImageType, max_width: u32) -> Option<String> {
        let kind = match image_type {
            media::ImageType::Poster => sdks::ImageType::Primary,
            media::ImageType::Backdrop => sdks::ImageType::Backdrop,
            media::ImageType::Logo => sdks::ImageType::Logo,
            media::ImageType::Thumb => sdks::ImageType::Thumb,
            // Jellyfin has no textless posters.
            media::ImageType::PosterTextless => return None,
        };
        // The picture of an episode is a still, wide: as a poster it shows its season's or its show's.
        let episode_poster = image_type == media::ImageType::Poster
            && media.media_type == media::MediaType::Episode
            && (media.images.parent_poster.is_some() || media.images.series_poster.is_some());
        // An episode or season without its own picture shows its season's or its show's.
        let owner = if media.images.tag(image_type).is_some() && !episode_poster {
            media.id
        } else {
            match image_type {
                media::ImageType::Thumb => media.images.parent_thumb?,
                media::ImageType::Backdrop => media.images.parent_backdrop?,
                media::ImageType::Poster => media.images.parent_poster.or(media.images.series_poster)?,
                _ => return None,
            }
        };
        let query = ImageQuery { max_width: Some(max_width), ..Default::default() };
        Some(sdks::item_image_url(&self.base, owner, kind, &query).to_string())
    }

    /// The cover of an album, by its id: what a track shows when it has no picture of its own.
    pub fn cover_url(&self, album: Uuid, width: u32) -> String {
        sdks::item_image_url(
            &self.base,
            album,
            sdks::ImageType::Primary,
            &ImageQuery {
                max_width: Some(width),
                ..Default::default()
            },
        )
        .to_string()
    }

    /// The portrait on the page of a person, `None` when there is none.
    pub fn portrait_url(&self, person: &Media) -> Option<String> {
        person.images.poster.as_ref()?;
        Some(
            sdks::item_image_url(
                &self.base,
                person.id,
                sdks::ImageType::Primary,
                &ImageQuery {
                    max_width: Some(480),
                    ..Default::default()
                },
            )
            .to_string(),
        )
    }

    /// A portrait of a cast or crew member, `None` when there is none.
    /// The still of an episode, its own picture (its `Primary`), `max_width` wide: what an
    /// episode's card shows on its show's page. `None` when it has none.
    pub fn still_url(&self, episode: &Media, max_width: u32) -> Option<String> {
        episode.images.poster.as_ref()?;
        let query = ImageQuery { max_width: Some(max_width), ..Default::default() };
        Some(sdks::item_image_url(&self.base, episode.id, sdks::ImageType::Primary, &query).to_string())
    }

    pub fn person_image_url(&self, person: &Person) -> Option<String> {
        person.has_image.then(|| {
            sdks::item_image_url(
                &self.base,
                person.id,
                sdks::ImageType::Primary,
                &ImageQuery {
                    max_width: Some(240),
                    ..Default::default()
                },
            )
            .to_string()
        })
    }

    pub async fn set_watched(&self, watched: bool, media: &Media) -> Result<()> {
        self.client
            .execute(sdks::TogglePlayed {
                user_id: self.user_id,
                item_id: media.id,
                played: watched,
            })
            .await?;
        crate::user_data::changed([Some(media.id), media.series_id, media.season_id]);
        Ok(())
    }

    pub async fn set_watchlist(&self, watchlist: bool, media: &Media) -> Result<()> {
        self.client
            .execute(sdks::ToggleWatchlist {
                user_id: self.user_id,
                item_id: media.id,
                watchlist,
            })
            .await?;
        crate::user_data::changed([Some(media.id), media.series_id, media.season_id]);
        Ok(())
    }

    pub async fn set_favorite(&self, favorite: bool, media: &Media) -> Result<()> {
        self.client
            .execute(sdks::ToggleFavorite {
                user_id: self.user_id,
                item_id: media.id,
                favorite,
            })
            .await?;
        crate::user_data::changed([Some(media.id), media.series_id, media.season_id]);
        Ok(())
    }

    /// The playlists of this user, by name.
    pub async fn playlists(&self) -> Result<Vec<(Uuid, String)>> {
        let found = self
            .client
            .execute(GetItems(GetItemsQuery {
                include_item_types: Some(vec![sdks::MediaType::Playlist]),
                sort_by: Some(vec![ItemSortBy::SortName]),
                limit: None,
                ..self.items_query()
            }))
            .await?;
        Ok(found.items.into_iter().map(|item| (item.id, item.name.unwrap_or_default())).collect())
    }

    /// The playlists of this user, by name, with how many items each has.
    pub async fn playlists_with_counts(&self) -> Result<Vec<(Uuid, String, i64)>> {
        let found = self
            .client
            .execute(GetItems(GetItemsQuery {
                include_item_types: Some(vec![sdks::MediaType::Playlist]),
                sort_by: Some(vec![ItemSortBy::SortName]),
                limit: None,
                fields: Some(vec![ItemFields::ChildCount, ItemFields::ItemCounts]),
                ..self.items_query()
            }))
            .await?;
        let counted = found.items.into_iter().map(|item| async move {
            let count = match item.child_count {
                Some(count) => count,
                // A server that does not say: its list is asked for, and that has the total.
                None => self
                    .client
                    .execute(sdks::GetPlaylistItems { playlist_id: item.id, user_id: self.user_id, start_index: 0, limit: 1 })
                    .await
                    .map(|list| list.total_record_count)
                    .unwrap_or(0),
            };
            (item.id, item.name.unwrap_or_default(), count)
        });
        Ok(futures::future::join_all(counted).await)
    }

    pub async fn add_to_playlist(&self, playlist: Uuid, media: &Media) -> Result<()> {
        self.client
            .execute(sdks::AddToPlaylist { playlist_id: playlist, user_id: self.user_id, item_id: media.id })
            .await?;
        invalidate_cache();
        Ok(())
    }

    /// Takes an item out of a playlist, by the entry it has there (`Media::playlist_item_id`).
    pub async fn remove_from_playlist(&self, playlist: Uuid, entry: &str) -> Result<()> {
        self.client
            .execute(sdks::RemoveFromPlaylist { playlist_id: playlist, entry_id: entry.to_string() })
            .await?;
        invalidate_cache();
        Ok(())
    }

    pub async fn create_playlist(&self, name: &str, media: &Media) -> Result<()> {
        self.client
            .execute(sdks::CreatePlaylist { name: name.to_string(), user_id: self.user_id, item_id: media.id })
            .await?;
        invalidate_cache();
        Ok(())
    }

    /// Lets the device that shows `code` sign in as this user.
    pub async fn authorize_quick_connect(&self, code: &str) -> Result<()> {
        let approved = self
            .client
            .execute(sdks::AuthorizeQuickConnect { code: code.to_string() })
            .await?;
        if approved {
            Ok(())
        } else {
            Err(anyhow!("The code was not accepted"))
        }
    }

    /// Changes the password. The server then ends every session of the user, this one too.
    pub async fn change_password(&self, current: &str, new: &str) -> Result<()> {
        self.client
            .execute(sdks::ChangePassword {
                user_id: self.user_id,
                current_pw: current.to_string(),
                new_pw: new.to_string(),
            })
            .await?;
        Ok(())
    }

    /// Sends a log to the server. Gives the name it is kept under there.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub async fn upload_log(&self, text: String) -> Result<String> {
        let answer = self.client.execute(sdks::ClientLogDocument { text }).await?;
        Ok(answer
            .get("FileName")
            .and_then(|name| name.as_str())
            .unwrap_or_default()
            .to_string())
    }

    pub fn api(&self) -> &sdks::Api {
        &self.client
    }

    /// An address on the server, relative to its base: for a transcode the server named.
    pub fn url_for(&self, path: &str) -> Result<url::Url> {
        Ok(self.base.join(path.trim_start_matches('/'))?)
    }

    /// The original file. A media element can follow the server's redirect to wherever the
    /// file lives, but only when it is handed this address as it is, not through `fetch`.
    pub fn direct_stream_url(&self, item_id: Uuid, source_id: Uuid) -> url::Url {
        sdks::video_stream_url(
            &self.base,
            item_id,
            &sdks::VideoStreamQuery {
                static_: Some(true),
                media_source_id: Some(source_id),
                ..Default::default()
            },
            Some(&self.token),
        )
    }

    /// A track as a media element takes it: the file as it is, with the key in the address.
    pub fn audio_url(&self, id: Uuid) -> url::Url {
        let mut url = self.base.join(&format!("audio/{id}/stream")).unwrap_or_else(|_| self.base.clone());
        url.query_pairs_mut().append_pair("Static", "true").append_pair("ApiKey", &self.token);
        url
    }

    // `queue_track` (a track for the music player) comes back with the music player (PLAN.md, step 3).

    /// A text subtitle as WebVTT.
    pub fn subtitle_url(&self, item_id: Uuid, source_id: Uuid, index: i64) -> url::Url {
        sdks::subtitle_url(&self.base, item_id, source_id, index, "vtt", Some(&self.token))
    }

    pub async fn genres(&self) -> Result<Vec<media::Genre>> {
        let res = self
            .client
            .execute(sdks::GetItemsFilters(sdks::ItemsFiltersQuery {
                include_item_types: Some(vec![sdks::MediaType::Series, sdks::MediaType::Movie]),
                // without a user this is 10x slower
                user_id: Some(self.user_id),
                ..Default::default()
            }))
            .await?;

        Ok(res
            .genres
            .unwrap_or_default()
            .into_iter()
            .map(|name| media::Genre {
                id: name.clone(),
                name,
            })
            .collect())
    }

    /// An item with everything its detail page shows, which lists leave out: the cast and crew
    /// can be over a hundred people.
    pub async fn media_details(&self, id: Uuid) -> Result<Option<Media>> {
        let mut query = self.items_query();
        query.ids = Some(vec![id]);
        query
            .fields
            .get_or_insert_default()
            .push(ItemFields::People);

        let res = self
            .client
            .execute(GetItems(query))
            .await?;
        res.items.into_iter().next().map(Media::try_from).transpose()
    }

    /// The versions of a movie or episode, with their streams. A separate request, as it is the
    /// heaviest part of an item and only needed to play it.
    pub async fn media_sources(&self, id: Uuid) -> Result<Vec<media::MediaSource>> {
        let mut query = self.items_query();
        query.ids = Some(vec![id]);
        query.fields = Some(vec![ItemFields::MediaSources]);
        let res = self.client.execute(GetItems(query)).await?;
        Ok(res
            .items
            .into_iter()
            .next()
            .map(Media::try_from)
            .transpose()?
            .map(|item| item.media_sources)
            .unwrap_or_default())
    }

    /// What the hero of the home screen shows: what the viewer is in the middle of, then their
    /// watchlist (the last added first, on a Remux server), then favorites they have not seen yet
    /// (random), and to fill up, popular things they have not seen yet (this week's most popular).
    /// Episodes stand for their show, whose Play button plays the episode that is next.
    ///
    /// Each of the last three is asked for `count` items, which is always enough: of the ones it
    /// gives, only those already in the hero are left out, and there are at most `count - room` of
    /// those, so `room` new ones are among the first `count`.
    ///
    /// The four are asked for at once, each for all the room there is: how much of them is needed
    /// is only known when the ones before are in, and waiting for that made the hero as slow as
    /// all the requests together. They are put together in the order above, and the hero is
    /// there as soon as it is full: what is still under way then is dropped.
    async fn hero(&self, q: &MediaQuery) -> Result<Vec<Media>> {
        let count = q.limit as usize;
        let wanted = |item: &Media| {
            (q.types.is_empty() || q.types.contains(&item.media_type))
                && (q.genres.is_empty() || q.genres.iter().any(|genre| item.genres.contains(genre)))
        };

        // What is being watched, with the shows that go with it.
        let resumable = async {
            let resumable = self
                .client
                .execute(GetItems(GetItemsQuery {
                    include_item_types: Some(vec![sdks::MediaType::Movie, sdks::MediaType::Episode]),
                    filters: Some(vec![ItemFilter::IsResumable]),
                    sort_by: Some(vec![ItemSortBy::DatePlayed]),
                    sort_order: Some(vec![SortOrder::Descending]),
                    limit: Some(count as u32 * 2),
                    ..self.items_query()
                }))
                .await?;

            // In that order, one entry per movie or show.
            let mut order: Vec<Uuid> = Vec::new();
            let mut movies: Vec<Media> = Vec::new();
            for item in resumable.items {
                let item = Media::try_from(item)?;
                let id = match item.media_type {
                    media::MediaType::Episode => match item.series_id {
                        Some(series) => series,
                        None => continue,
                    },
                    _ => item.id,
                };
                if order.contains(&id) {
                    continue;
                }
                order.push(id);
                if item.id == id {
                    movies.push(item);
                }
            }

            // The shows themselves are one more request.
            let show_ids: Vec<Uuid> = order.iter().copied().filter(|id| !movies.iter().any(|m| m.id == *id)).collect();
            let mut known: Vec<Media> = movies;
            if !show_ids.is_empty() {
                let mut query = self.items_query();
                query.ids = Some(show_ids);
                let shows = self.client.execute(GetItems(query)).await?;
                for item in shows.items {
                    known.push(Media::try_from(item)?);
                }
            }
            let mut items: Vec<Media> = order
                .iter()
                .filter_map(|id| known.iter().find(|item| item.id == *id).cloned())
                .filter(|item| wanted(item))
                .collect();
            items.truncate(count);
            Ok::<_, anyhow::Error>(items.into_iter().map(Ok).collect::<Vec<Result<Media>>>())
        };

        // The watchlist, which only a Remux server has, the last added first.
        let watchlist = async {
            if !self.is_remux().await {
                return Ok(Vec::new());
            }
            let res = self
                .client
                .execute(GetItems(GetItemsQuery {
                    include_item_types: Some(q.types.iter().cloned().map(Into::into).collect()),
                    genres: (!q.genres.is_empty()).then(|| q.genres.clone()),
                    is_watchlist: Some(true),
                    sort_by: Some(vec![ItemSortBy::DateWatchlisted]),
                    sort_order: Some(vec![SortOrder::Descending]),
                    limit: Some(count as u32),
                    ..self.items_query()
                }))
                .await?;
            Ok::<_, anyhow::Error>(res.items.into_iter().map(Media::try_from).collect::<Vec<Result<Media>>>())
        };

        // Favorites that have not been watched, in a random order, for all the room that is left:
        // popular things are only to fill up what these leave.
        let favorites = async {
            let res = self
                .client
                .execute(GetItems(GetItemsQuery {
                    include_item_types: Some(q.types.iter().cloned().map(Into::into).collect()),
                    genres: (!q.genres.is_empty()).then(|| q.genres.clone()),
                    // Two filters in one request are not understood by the server.
                    is_favorite: Some(true),
                    filters: Some(vec![ItemFilter::IsUnplayed]),
                    sort_by: Some(vec![ItemSortBy::Random]),
                    limit: Some(count as u32),
                    ..self.items_query()
                }))
                .await?;
            Ok::<_, anyhow::Error>(res.items.into_iter().map(Media::try_from).collect::<Vec<Result<Media>>>())
        };

        // What is popular this week, that has not been watched.
        let popular = async {
            let res = self
                .client
                .execute(GetItems(GetItemsQuery {
                    include_item_types: Some(q.types.iter().cloned().map(Into::into).collect()),
                    genres: (!q.genres.is_empty()).then(|| q.genres.clone()),
                    filters: Some(vec![ItemFilter::IsUnplayed]),
                    // What the server's viewers watch this week.
                    sort_by: Some(vec![ItemSortBy::PopularityWeek]),
                    sort_order: Some(vec![SortOrder::Descending]),
                    limit: Some(count as u32),
                    ..self.items_query()
                }))
                .await?;
            Ok::<_, anyhow::Error>(res.items.into_iter().map(Media::try_from).collect::<Vec<Result<Media>>>())
        };

        // All four are under way; their answers are taken in the order above, and as soon as the
        // hero is full the rest is let go, so a slow one that is not needed does not hold it up.
        let mut lists = FuturesOrdered::new();
        lists.push_back(resumable.boxed_local());
        lists.push_back(watchlist.boxed_local());
        lists.push_back(favorites.boxed_local());
        lists.push_back(popular.boxed_local());

        let Some(resumable) = lists.next().await else {
            return Ok(Vec::new());
        };
        let mut items: Vec<Media> = resumable?.into_iter().collect::<Result<_>>()?;
        while items.len() < count {
            let Some(found) = lists.next().await else {
                break;
            };
            let room = count - items.len();
            let mut added = 0;
            for item in found? {
                if added >= room {
                    break;
                }
                let item = item?;
                if !items.iter().any(|existing| existing.id == item.id) {
                    items.push(item);
                    added += 1;
                }
            }
        }
        Ok(items)
    }

    pub async fn media(&self, q: &MediaQuery) -> Result<Vec<Media>> {
        if let Some(playlist) = q.playlist {
            let res = self
                .client
                .execute(sdks::GetPlaylistItems {
                    playlist_id: playlist,
                    user_id: self.user_id,
                    start_index: q.offset,
                    limit: q.limit,
                })
                .await?;
            return res.items.into_iter().map(Media::try_from).collect();
        }
        if q.catalog == Some(CatalogId::Hero) {
            // One page: it is all of it.
            return if q.offset == 0 {
                let hero = self.hero(q).await;
                if let Err(err) = &hero {
                    log::error!("hero: {err:?}");
                }
                hero
            } else {
                Ok(Vec::new())
            };
        }
        if q.catalog == Some(CatalogId::NextUp) {
            let res = self
                .client
                .execute(sdks::GetNextUp(sdks::NextUpQuery {
                    user_id: Some(self.user_id),
                    limit: Some(q.limit),
                    start_index: Some(q.offset),
                    ..Default::default()
                }))
                .await?;
            return res.items.into_iter().map(Media::try_from).collect();
        }
        if let (true, Some(CatalogId::Collection(library))) = (q.latest, &q.catalog) {
            // One page: it is all of it. What has been watched is left out, as Jellyfin does.
            if q.offset > 0 {
                return Ok(Vec::new());
            }
            let res = self
                .client
                .execute(sdks::GetItemsLatest(sdks::ItemsLatestQuery {
                    user_id: Some(self.user_id),
                    parent_id: Some(*library),
                    fields: self.items_query().fields,
                    // A mixed library has no kind of item to ask for.
                    include_item_types: (!q.types.is_empty()).then(|| q.types.iter().cloned().map(Into::into).collect()),
                    is_played: Some(false),
                    limit: Some(q.limit.max(40)),
                    ..Default::default()
                }))
                .await?;
            let items = res.into_iter().map(Media::try_from).collect::<Result<Vec<_>>>()?;
            return Ok(items
                .into_iter()
                .filter(|item| q.types.is_empty() || q.types.contains(&item.media_type))
                .collect());
        }
        let mut filters = match q.catalog {
            Some(CatalogId::Favorites) => vec![ItemFilter::IsFavorite],
            _ => vec![],
        };
        if q.hide_watched {
            filters.push(ItemFilter::IsUnplayed);
        }

        let (sort_by, sort_order) = match q.sort.filter(|_| q.search_query.is_none()) {
            Some(sort) => (
                Some(sort.item_sorts()),
                Some(vec![if q.descending { SortOrder::Descending } else { SortOrder::Ascending }]),
            ),
            None => (None, None),
        };

        let parent_id = match q.catalog {
            Some(CatalogId::Collection(id)) => Some(id),
            _ => q.parent,
        };

        let res = self
            .client
            .execute(GetItems(GetItemsQuery {
                include_item_types: Some(q.types.iter().cloned().map(Into::into).collect()),
                recursive: q.recursive,
                parent_id,
                limit: Some(q.limit),
                start_index: Some(q.offset),
                genres: (!q.genres.is_empty()).then(|| q.genres.clone()),
                search_term: q.search_query.clone(),
                filters: (!filters.is_empty()).then_some(filters),
                person_ids: q.person.map(|id| vec![id.to_string()]),
                artist_ids: q.artist.map(|id| vec![id]),
                album_artist_ids: q.album_artist.map(|id| vec![id]),
                sort_by,
                sort_order,
                ..self.items_query()
            }))
            .await?;
        // An item of a kind the app has no page for (a folder, a trailer) is left out, and does
        // not take the whole list with it.
        Ok(res.items.into_iter().filter_map(|item| Media::try_from(item).ok()).collect())
    }

    /// The next episode to watch of a series.
    pub async fn nextup(&self, series: &Media) -> Result<Vec<Media>> {
        let res = self
            .client
            .execute(sdks::GetNextUp(sdks::NextUpQuery {
                user_id: Some(self.user_id),
                series_id: Some(series.id),
                limit: Some(1),
                ..Default::default()
            }))
            .await?;
        res.items.into_iter().map(Media::try_from).collect()
    }

    /// The libraries the user can browse, in the server's order.
    pub async fn libraries(&self) -> Result<Vec<media::Library>> {
        Ok(Self::libraries_of(self.user_views().await?))
    }

    /// What the server says the user's views are: the one request that the libraries and the rows
    /// of the home screen are both made from.
    pub async fn user_views(&self) -> Result<Vec<sdks::BaseItemDto>> {
        Ok(self
            .client
            .execute(sdks::GetUserViews { user_id: self.user_id })
            .await?
            .items)
    }

    /// The libraries in `views`, in the server's order.
    pub fn libraries_of(views: Vec<sdks::BaseItemDto>) -> Vec<media::Library> {
        views
            .into_iter()
            // The playlists have a card of their own on the Media page.
            .filter(|item| item.collection_type != Some(sdks::CollectionType::Playlists))
            .filter_map(|item| {
                Some(media::Library {
                    id: item.id,
                    name: item.name?,
                    has_image: item.image_tags.is_some_and(|tags| tags.primary.is_some()),
                    live_tv: item.collection_type == Some(sdks::CollectionType::Livetv),
                })
            })
            .collect()
    }

    /// Whether the server forces its own order of the libraries on everyone, which its public info
    /// says (an older server doesn't, and then it is found out when an order is saved).
    pub async fn library_order_forced(&self) -> bool {
        let info = async {
            sdks::public_info(self.base.as_str()).await
        };
        info.await
            .and_then(|info| info.get("RemuxForceDefaultViewOrder")?.as_bool())
            .unwrap_or(false)
    }

    /// Saves the order the user wants their libraries in, and says whether the server kept to it.
    /// A server can force its own order on everyone (Remux: `ForceDefaultViewOrder`): it then
    /// accepts the order and shows another, which is how a client finds out.
    pub async fn set_library_order(&self, order: &[Uuid]) -> Result<bool> {
        let user = self.client.execute(sdks::GetCurrentUser).await?;
        let mut config = user.configuration.unwrap_or_default();
        config.ordered_views = order.iter().map(|id| id.to_string()).collect();
        self.client
            .execute(sdks::UpdateUserConfiguration { user_id: self.user_id, config })
            .await?;
        // The views come in another order now.
        invalidate_cache();
        let shown: Vec<Uuid> = self.libraries().await?.into_iter().filter(|library| !library.live_tv).map(|library| library.id).collect();
        // Libraries the user may not see, or that are no libraries here, are left out of both.
        let wanted: Vec<Uuid> = order.iter().copied().filter(|id| shown.contains(id)).collect();
        Ok(shown.iter().copied().filter(|id| order.contains(id)).eq(wanted.iter().copied()))
    }

    /// The banner of a library, `None` when it has none.
    pub fn library_image_url(&self, library: &media::Library) -> Option<String> {
        library.has_image.then(|| {
            sdks::item_image_url(
                &self.base,
                library.id,
                sdks::ImageType::Primary,
                &ImageQuery { max_width: Some(960), ..Default::default() },
            )
            .to_string()
        })
    }

    /// Whether the user has Live TV: among their views is one of that kind, which the server only
    /// adds when there are channels to watch and the user may watch them.
    pub async fn has_live_tv(&self) -> Result<bool> {
        let res = self.client.execute(sdks::GetUserViews { user_id: self.user_id }).await?;
        Ok(res.items.iter().any(|item| item.collection_type == Some(sdks::CollectionType::Livetv)))
    }

    /// Whether the server has music for the user: a view of that kind, else any album at all (a
    /// Remux server may not list one). Music is only offered where there is some.
    pub async fn has_music(&self) -> Result<bool> {
        let res = self.client.execute(sdks::GetUserViews { user_id: self.user_id }).await?;
        if res.items.iter().any(|item| item.collection_type == Some(sdks::CollectionType::Music)) {
            return Ok(true);
        }
        let query = MediaQuery::builder().limit(1).types(vec![media::MediaType::Album]).build();
        Ok(!self.media(&query).await?.is_empty())
    }

    /// The rows of the home screen from `views`: the user's libraries, in the order the server has
    /// for them (the user's own order, else the one the admin set). Libraries of what the app
    /// cannot show (music, playlists, collections of collections) are left out.
    pub fn catalogs_of(views: Vec<sdks::BaseItemDto>) -> Vec<Catalog> {
        let mut next_up = Catalog::new(CatalogId::NextUp, "Next Up");
        next_up.card_variant = media::CardVariant::Landscape;
        let libraries = views
            .into_iter()
            .filter(|item| {
                matches!(
                    item.collection_type,
                    None | Some(sdks::CollectionType::Movies | sdks::CollectionType::Tvshows | sdks::CollectionType::Mixed)
                )
            })
            .filter_map(|item| {
                let kind = match item.collection_type {
                    Some(sdks::CollectionType::Movies) => media::LibraryKind::Movies,
                    Some(sdks::CollectionType::Tvshows) => media::LibraryKind::Shows,
                    _ => media::LibraryKind::Mixed,
                };
                let mut catalog = Catalog::new(CatalogId::Collection(item.id), item.name?);
                catalog.library = Some(kind);
                Some(catalog)
            });
        std::iter::once(next_up).chain(libraries).collect()
    }

    /// Base query for item lookups: recursive, with the fields the UI needs.
    fn items_query(&self) -> GetItemsQuery {
        GetItemsQuery {
            user_id: Some(self.user_id),
            recursive: true,
            limit: Some(25),
            fields: Some(vec![
                ItemFields::ProviderIds,
                ItemFields::Genres,
                ItemFields::Overview,
                // For the page of a title, which opens with what the list has (see `media::recent`).
                // Not People: that is the cast, tens of persons for each of the items of a list.
                ItemFields::Studios,
                ItemFields::Taglines,
                ItemFields::PremiereDate,
                ItemFields::ProductionLocations,
                // Not MediaSources: a title can have dozens, each with its streams, which makes
                // an item many times as big. They are asked for when playing, see `media_sources`.
            ]),
            ..Default::default()
        }
    }
}

/// `GET /LiveTv/Programs`, and `/LiveTv/Programs/Recommended` for what is on now. Neither is in the
/// SDK; the filters go in the query as the server names them.
#[derive(Clone)]
struct GetPrograms {
    recommended: bool,
    params: Vec<(String, String)>,
}

impl Endpoint for GetPrograms {
    type Output = crate::sdks::QueryResult<crate::sdks::BaseItemDto>;

    fn path(&self) -> String {
        if self.recommended {
            "/livetv/programs/recommended".to_string()
        } else {
            "/livetv/programs".to_string()
        }
    }

    fn query_params(&self) -> impl serde::Serialize + '_ {
        &self.params
    }
}

/// `POST /Sessions/Logout`: the token stops being good, for every part that uses it.
#[derive(Clone)]
struct Logout;

impl Endpoint for Logout {
    type Output = serde_json::Value;

    fn path(&self) -> String {
        "/sessions/logout".to_string()
    }

    fn method(&self) -> http::Method {
        http::Method::POST
    }
}

impl JellyfinServer {
    /// Ends the session at the server too, as Jellyfin does. An error is of no matter: the app
    /// is logged out either way.
    pub async fn log_out(&self) {
        let _ = self.client.execute(Logout).await;
    }

    async fn programs(&self, recommended: bool, params: Vec<(&str, String)>) -> Result<Vec<media::Program>> {
        let mut query = vec![("userId".to_string(), self.user_id.to_string())];
        query.extend(params.into_iter().map(|(name, value)| (name.to_string(), value)));
        let res = self.client.execute(GetPrograms { recommended, params: query }).await?;
        // A programme that is missing something is left out, not an error for the rest.
        Ok(res.items.into_iter().filter_map(|item| media::Program::try_from(item).ok()).collect())
    }

    /// What is on the air now, one for each channel.
    pub async fn programs_on_now(&self, limit: u32) -> Result<Vec<media::Program>> {
        self.programs(true, vec![("limit", limit.to_string()), ("isAiring", "true".to_string())]).await
    }

    /// What is on, and what is coming, of one kind: one for each channel, the first of it.
    pub async fn programs_of_kind(&self, kind: media::ProgramKind, limit: u32) -> Result<Vec<media::Program>> {
        let all = self
            .programs(
                false,
                vec![
                    (kind.param(), "true".to_string()),
                    ("hasAired", "false".to_string()),
                    ("limit", (limit * 4).to_string()),
                ],
            )
            .await?;
        let mut channels = std::collections::HashSet::new();
        let mut programs: Vec<media::Program> = all.into_iter().filter(|p| channels.insert(p.channel_id)).collect();
        programs.sort_by_key(|p| p.start);
        programs.truncate(limit as usize);
        Ok(programs)
    }

    /// What the channels have on between two moments, for the guide.
    pub async fn programs_between(
        &self,
        channels: &[Uuid],
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<media::Program>> {
        let ids = channels.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        self.programs(
            false,
            vec![
                ("channelIds", ids),
                ("minEndDate", from.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
                ("maxStartDate", to.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
                ("limit", "5000".to_string()),
            ],
        )
        .await
    }

    /// The URL of a channel's logo, for the programmes that are on it.
    pub fn channel_logo(&self, channel: Uuid) -> String {
        sdks::item_image_url(&self.base, channel, sdks::ImageType::Primary, &ImageQuery::default()).to_string()
    }
}
