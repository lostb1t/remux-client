//! The Remux server SDK, which also speaks the Jellyfin API.
pub use remux_sdks::remux::*;
pub use remux_sdks::Endpoint;

/// Takes the `null`s out of a JSON value, in objects and in what is inside them.
fn without_nulls(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.retain(|_, field| !field.is_null());
            map.values_mut().for_each(without_nulls);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(without_nulls),
        _ => {}
    }
}

/// An endpoint that is sent without the `null`s in its body.
#[derive(Clone)]
pub struct WithoutNulls<E>(pub E);

impl<E: Endpoint + Clone> Endpoint for WithoutNulls<E> {
    type Output = E::Output;

    fn path(&self) -> String {
        self.0.path()
    }

    fn query(&self) -> Vec<(String, String)> {
        self.0.query()
    }

    fn method(&self) -> http::Method {
        self.0.method()
    }

    fn headers(&self) -> http::HeaderMap {
        self.0.headers()
    }

    fn body(&self) -> remux_sdks::Body {
        match self.0.body() {
            // A field that is not there is not sent as `null`: Jellyfin 12 turns a request down
            // that has one for a field that may not be (the video codec of an audio stream), and
            // before that it read it as not there.
            remux_sdks::Body::Json(mut value) => {
                without_nulls(&mut value);
                remux_sdks::Body::Json(value)
            }
            other => other,
        }
    }

    fn cache_options(&self) -> Option<remux_sdks::CacheOptions> {
        self.0.cache_options()
    }

    fn should_cache(&self, response: &Self::Output) -> Option<std::time::Duration> {
        self.0.should_cache(response)
    }
}

/// Why a request to the server has no answer to read.
#[derive(Debug)]
pub enum ApiError {
    /// The server never answered: not there, or not at this address (as opposed to refusing).
    Unreachable(String),
    /// The token is not, or no longer, good.
    Unauthorized,
    /// Another status than a success.
    Http { status: u16, path: String },
    /// An answer that is not what the endpoint gives.
    Json { path: String, source: serde_json::Error },
    /// An address that is not one.
    Url(url::ParseError),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Unreachable(why) => write!(f, "the server could not be reached: {why}"),
            ApiError::Unauthorized => write!(f, "unauthorized"),
            ApiError::Http { status, path } => write!(f, "{path}: HTTP {status}"),
            ApiError::Json { path, source } => write!(f, "{path}: {source}"),
            ApiError::Url(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ApiError {}

/// The client of the server: the endpoints of the SDK, sent with NSH's `fetch` (the platform's
/// network stack, the browser's on the web), without the `null`s in what it sends.
#[derive(Clone)]
pub struct Api {
    base: url::Url,
    auth: JellyfinAuth,
}

impl Api {
    pub fn new(host: &str, auth: JellyfinAuth) -> Result<Self, ApiError> {
        let mut base = url::Url::parse(host).map_err(ApiError::Url)?;
        // A base without a trailing slash would lose its last segment to `join`.
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        Ok(Self { base, auth })
    }

    /// Jellyfin's `Authorization` header: what the app is, and the token.
    fn authorization(&self) -> String {
        let auth = &self.auth;
        let mut value = format!(
            r#"MediaBrowser Client="{}", Device="{}", DeviceId="{}", Version="{}""#,
            auth.client, auth.device, auth.device_id, auth.version
        );
        if let Some(token) = &auth.token {
            value.push_str(&format!(r#", Token="{token}""#));
        }
        value
    }

    pub async fn execute<E: Endpoint + Clone>(&self, endpoint: E) -> Result<E::Output, ApiError> {
        let endpoint = WithoutNulls(endpoint);
        let path = endpoint.path();
        let mut url = self.base.join(path.trim_matches('/')).map_err(ApiError::Url)?;
        // `query()` gives pairs that are already percent-encoded: joined as they are.
        let query = endpoint.query();
        if !query.is_empty() {
            let query: Vec<String> = query.iter().map(|(key, value)| format!("{key}={value}")).collect();
            url.set_query(Some(&query.join("&")));
        }
        let mut request = nsh_core::platform::HttpRequest::new(endpoint.method().as_str(), url.as_str())
            .header("Authorization", self.authorization())
            .header("Accept", "application/json");
        for (name, value) in endpoint.headers().iter() {
            if let Ok(value) = value.to_str() {
                request = request.header(name.as_str(), value);
            }
        }
        request = match endpoint.body() {
            remux_sdks::Body::Empty => request,
            remux_sdks::Body::Json(value) => request
                .header("Content-Type", "application/json")
                .body(serde_json::to_vec(&value).unwrap_or_default()),
            remux_sdks::Body::Form(pairs) => request
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(serde_urlencoded::to_string(&pairs).unwrap_or_default()),
            remux_sdks::Body::Text(text) => request.header("Content-Type", "text/plain").body(text),
            remux_sdks::Body::Bytes(bytes) => request.body(bytes),
        };
        let response = nsh_core::prelude::fetch(request).await.map_err(ApiError::Unreachable)?;
        match response.status() {
            401 => return Err(ApiError::Unauthorized),
            status if !response.ok() => return Err(ApiError::Http { status, path }),
            _ => {}
        }
        // An answer without a body (a 204) is read as `null`, which `()` and `Option` take.
        let body: &[u8] = if response.body.is_empty() { b"null" } else { &response.body };
        serde_json::from_slice(body).map_err(|source| ApiError::Json { path, source })
    }
}

/// What the server says of itself without a login (`/System/Info/Public`): its name, whether it is
/// a Remux one. `None` when it does not answer.
pub async fn public_info(host: &str) -> Option<serde_json::Value> {
    let url = format!("{}/System/Info/Public", host.trim_end_matches('/'));
    let response = nsh_core::prelude::fetch(url.as_str()).await.ok()?;
    if !response.ok() {
        return None;
    }
    serde_json::from_slice(&response.body).ok()
}

/// `GET /UserViews`: the libraries a user sees, in the order the server wants them.
#[derive(Debug, Clone)]
pub struct GetUserViews {
    pub user_id: uuid::Uuid,
}

impl Endpoint for GetUserViews {
    type Output = QueryResult<BaseItemDto>;

    fn path(&self) -> String {
        "/userviews".into()
    }

    fn query_params(&self) -> impl serde::Serialize + '_ {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "PascalCase")]
        struct Query {
            user_id: uuid::Uuid,
        }
        Query { user_id: self.user_id }
    }
}

/// `POST /Users/{id}/Password`: the server signs the user out of every device afterwards.
#[derive(Debug, Clone)]
pub struct ChangePassword {
    pub user_id: uuid::Uuid,
    pub current_pw: String,
    pub new_pw: String,
}

impl Endpoint for ChangePassword {
    type Output = ();

    fn path(&self) -> String {
        format!("/users/{}/password", self.user_id)
    }

    fn method(&self) -> http::Method {
        http::Method::POST
    }

    fn body(&self) -> remux_sdks::Body {
        remux_sdks::Body::Json(serde_json::json!({
            "CurrentPw": self.current_pw,
            "NewPw": self.new_pw,
        }))
    }
}

/// `POST /ClientLog/Document`: a log of this client, which the server keeps in its own log folder.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Debug, Clone)]
pub struct ClientLogDocument {
    pub text: String,
}

impl Endpoint for ClientLogDocument {
    type Output = serde_json::Value;

    fn path(&self) -> String {
        "/clientlog/document".into()
    }

    fn method(&self) -> http::Method {
        http::Method::POST
    }

    fn headers(&self) -> http::HeaderMap {
        let mut headers = http::HeaderMap::new();
        headers.insert(http::header::CONTENT_TYPE, http::HeaderValue::from_static("text/plain"));
        headers
    }

    fn body(&self) -> remux_sdks::Body {
        remux_sdks::Body::Text(self.text.clone())
    }
}

/// `POST /Sessions/Capabilities/Full`: what this device can play, which the server keeps on the
/// device to sort the versions of a title by. It is what a first playback would tell it.
#[derive(Debug, Clone)]
pub struct PostFullCapabilities {
    pub device_profile: DeviceProfile,
}

impl Endpoint for PostFullCapabilities {
    type Output = ();

    fn path(&self) -> String {
        "/sessions/capabilities/full".into()
    }

    fn method(&self) -> http::Method {
        http::Method::POST
    }

    fn body(&self) -> remux_sdks::Body {
        let capabilities = ClientCapabilitiesDto {
            playable_media_types: vec!["Video".into()],
            supports_persistent_identifier: true,
            device_profile: Some(self.device_profile.clone()),
            ..Default::default()
        };
        remux_sdks::Body::Json(serde_json::to_value(capabilities).unwrap_or_default())
    }
}

/// `GET /QuickConnect/Enabled`: whether a device may sign in with a code that another approves.
#[derive(Debug, Clone)]
pub struct QuickConnectEnabled;

impl Endpoint for QuickConnectEnabled {
    type Output = bool;

    fn path(&self) -> String {
        "/quickconnect/enabled".into()
    }
}

/// `POST /QuickConnect/Initiate`: a code to show, and a secret to ask after it with.
#[derive(Debug, Clone)]
pub struct InitiateQuickConnect;

impl Endpoint for InitiateQuickConnect {
    type Output = QuickConnectResult;

    fn path(&self) -> String {
        "/quickconnect/initiate".into()
    }

    fn method(&self) -> http::Method {
        http::Method::POST
    }
}

/// `GET /QuickConnect/Connect`: how a request stands, and whether it has been approved.
#[derive(Debug, Clone)]
pub struct GetQuickConnect {
    pub secret: String,
}

impl Endpoint for GetQuickConnect {
    type Output = QuickConnectResult;

    fn path(&self) -> String {
        "/quickconnect/connect".into()
    }

    fn query_params(&self) -> impl serde::Serialize + '_ {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "PascalCase")]
        struct Query<'a> {
            secret: &'a str,
        }
        Query { secret: &self.secret }
    }
}

/// `POST /Users/AuthenticateWithQuickConnect`: the session of an approved request.
#[derive(Debug, Clone)]
pub struct SignInWithQuickConnect {
    pub secret: String,
}

impl Endpoint for SignInWithQuickConnect {
    type Output = AuthenticationResult;

    fn path(&self) -> String {
        "/users/authenticatewithquickconnect".into()
    }

    fn method(&self) -> http::Method {
        http::Method::POST
    }

    fn body(&self) -> remux_sdks::Body {
        remux_sdks::Body::Json(serde_json::json!({ "Secret": self.secret }))
    }
}

/// `POST /Playlists/{id}/Items`: puts an item at the end of a playlist.
#[derive(Debug, Clone)]
pub struct AddToPlaylist {
    pub playlist_id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub item_id: uuid::Uuid,
}

impl Endpoint for AddToPlaylist {
    type Output = ();

    fn path(&self) -> String {
        format!("/playlists/{}/items", self.playlist_id)
    }

    fn method(&self) -> http::Method {
        http::Method::POST
    }

    fn query_params(&self) -> impl serde::Serialize + '_ {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Query {
            ids: uuid::Uuid,
            user_id: uuid::Uuid,
        }
        Query { ids: self.item_id, user_id: self.user_id }
    }
}

/// `POST /Playlists`: a new playlist with one item in it.
#[derive(Debug, Clone)]
pub struct CreatePlaylist {
    pub name: String,
    pub user_id: uuid::Uuid,
    pub item_id: uuid::Uuid,
}

impl Endpoint for CreatePlaylist {
    type Output = serde_json::Value;

    fn path(&self) -> String {
        "/playlists".into()
    }

    fn method(&self) -> http::Method {
        http::Method::POST
    }

    fn body(&self) -> remux_sdks::Body {
        remux_sdks::Body::Json(serde_json::json!({
            "Name": self.name,
            "Ids": [self.item_id],
            "UserId": self.user_id,
        }))
    }
}

/// `GET /Playlists/{id}/Items`: what is in a playlist, in its order.
#[derive(Debug, Clone)]
pub struct GetPlaylistItems {
    pub playlist_id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub start_index: u32,
    pub limit: u32,
}

impl Endpoint for GetPlaylistItems {
    type Output = QueryResult<BaseItemDto>;

    fn path(&self) -> String {
        format!("/playlists/{}/items", self.playlist_id)
    }

    fn query_params(&self) -> impl serde::Serialize + '_ {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Query {
            user_id: uuid::Uuid,
            start_index: u32,
            limit: u32,
            fields: &'static str,
        }
        Query { user_id: self.user_id, start_index: self.start_index, limit: self.limit, fields: "Overview,Genres,ProviderIds" }
    }
}

/// `DELETE /Playlists/{id}/Items`: takes an entry out of a playlist. The entry is the
/// `PlaylistItemId` of the item as the playlist lists it, not the id of the item itself.
#[derive(Debug, Clone)]
pub struct RemoveFromPlaylist {
    pub playlist_id: uuid::Uuid,
    pub entry_id: String,
}

impl Endpoint for RemoveFromPlaylist {
    type Output = ();

    fn path(&self) -> String {
        format!("/playlists/{}/items", self.playlist_id)
    }

    fn method(&self) -> http::Method {
        http::Method::DELETE
    }

    fn query_params(&self) -> impl serde::Serialize + '_ {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Query<'a> {
            entry_ids: &'a str,
        }
        Query { entry_ids: &self.entry_id }
    }
}
