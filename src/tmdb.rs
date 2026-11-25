//! Textless posters from TMDB.
//!
//! Needs a TMDB "API Read Access Token" at build time (`TMDB_READ_TOKEN`). Without one
//! the lookup is skipped and the regular artwork is used.

use anyhow::Result;
use serde::Deserialize;

const TOKEN: Option<&str> = option_env!("TMDB_READ_TOKEN");

#[derive(Deserialize)]
struct Images {
    posters: Vec<Poster>,
}

#[derive(Deserialize)]
struct Poster {
    file_path: String,
    iso_639_1: Option<String>,
}

/// URL of the first poster without a language (so without text), if there is one.
/// `kind` is TMDB's path segment: `movie` or `tv`.
pub async fn textless_poster(kind: &str, tmdb_id: u32) -> Result<Option<String>> {
    let Some(token) = TOKEN else {
        return Ok(None);
    };
    // Remembered: the hero of a title's page asks for the same one as the hero it was opened from,
    // and a request to TMDB (with its connection set up) before the picture can start to load
    // kept the page without a backdrop for a second or more on a phone.
    let key = (kind.to_string(), tmdb_id);
    if let Some(known) = KNOWN.lock().ok().and_then(|known| known.get(&key).cloned()) {
        return Ok(known);
    }
    let request = nsh_core::platform::HttpRequest::get(format!(
        "https://api.themoviedb.org/3/{kind}/{tmdb_id}/images?include_image_language=null"
    ))
    .header("Authorization", format!("Bearer {token}"));
    let response = nsh_core::prelude::fetch(request).await.map_err(anyhow::Error::msg)?;
    if !response.ok() {
        anyhow::bail!("TMDB: HTTP {}", response.status());
    }
    let images: Images = serde_json::from_slice(&response.body)?;

    let url = images
        .posters
        .into_iter()
        .find(|p| p.iso_639_1.is_none())
        .map(|p| format!("https://image.tmdb.org/t/p/w780{}", p.file_path));
    if let Ok(mut known) = KNOWN.lock() {
        // Not unbounded: when it is full, all of it goes and is asked for again when needed.
        if known.len() >= 500 {
            known.clear();
        }
        known.insert(key, url.clone());
    }
    Ok(url)
}

/// The answers of TMDB so far, by kind and id.
static KNOWN: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<(String, u32), Option<String>>>> =
    std::sync::LazyLock::new(Default::default);
