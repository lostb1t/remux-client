//! The user's settings, kept in NSH's storage and the same everywhere in the app.

use nsh_core::prelude::*;
use nsh_core::storage::Storage;
use serde::{Deserialize, Serialize};

const SETTINGS_KEY: &str = "settings";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Settings {
    /// Hide items that have been watched.
    pub filter_watched: bool,
    /// Not to show HDR as HDR, where the screen can: the server then makes it SDR.
    pub hdr_off: bool,
    /// Tell the server how fast the line to it is, so it picks the versions of a title that fit.
    pub auto_quality: bool,
}

/// What is kept in the signal: whether it was read from storage yet.
#[derive(Clone, PartialEq, Default)]
struct Loaded(Option<Settings>);

/// The settings (tracked where they are read).
pub fn settings() -> Settings {
    let loaded = global::<Loaded>();
    if let Some(settings) = loaded.with_untracked(|loaded| loaded.0.clone()) {
        return loaded.with(|loaded| loaded.0.clone()).unwrap_or(settings);
    }
    let settings = Storage::get_item(SETTINGS_KEY)
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    loaded.set(Loaded(Some(settings)));
    loaded.with(|loaded| loaded.0.clone()).unwrap_or_default()
}

/// Changes the settings, and keeps them.
pub fn update(change: impl FnOnce(&mut Settings)) {
    let mut next = untracked(settings);
    change(&mut next);
    if let Ok(text) = serde_json::to_string(&next) {
        let _ = Storage::set_item(SETTINGS_KEY, text);
    }
    global::<Loaded>().set(Loaded(Some(next)));
}
