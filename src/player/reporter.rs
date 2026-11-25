use std::cell::Cell;
use std::sync::Arc;

use log::debug;
use uuid::Uuid;

use super::api::Stage;
use crate::sdks::{
    PlayMethod, PlaybackInfo, ReportPlaybackProgress, ReportPlaybackStart, ReportPlaybackStopped,
};
use crate::server::JellyfinServer;

/// Tells the server how one playback is going.
pub struct Reporter {
    server: Arc<JellyfinServer>,
    item_id: Uuid,
    source_id: Uuid,
    play_session_id: Option<String>,
    method: PlayMethod,
    stopped: Cell<bool>,
    /// The season and the show the item is in: what shows them is told when it stops.
    parents: Vec<Uuid>,
}

impl Reporter {
    pub fn new(
        server: Arc<JellyfinServer>,
        item_id: Uuid,
        source_id: Uuid,
        play_session_id: Option<String>,
        method: PlayMethod,
    ) -> Self {
        Self {
            server,
            item_id,
            source_id,
            play_session_id,
            method,
            stopped: Cell::new(false),
            parents: Vec::new(),
        }
    }

    /// The season and the show the item is in, told along when it stops.
    pub fn with_parents(mut self, parents: impl IntoIterator<Item = Option<Uuid>>) -> Self {
        self.parents = parents.into_iter().flatten().collect();
        self
    }

    fn info(&self, position_ms: f64, paused: bool, audio: Option<i64>, subtitle: Option<i64>) -> PlaybackInfo {
        PlaybackInfo {
            item_id: self.item_id,
            // As text, without dashes, which is how Jellyfin has the id of a version.
            media_source_id: Some(self.source_id.simple().to_string()),
            play_session_id: self.play_session_id.clone(),
            position_ticks: Some((position_ms * 10_000.0) as i64),
            is_paused: paused,
            can_seek: true,
            play_method: Some(self.method.clone()),
            audio_stream_index: audio.map(|a| a as i32),
            subtitle_stream_index: subtitle.map(|s| s as i32),
            ..Default::default()
        }
    }

    pub async fn report(
        &self,
        stage: Stage,
        position_ms: f64,
        paused: bool,
        audio: Option<i64>,
        subtitle: Option<i64>,
    ) {
        if self.stopped.get() {
            return;
        }
        if matches!(stage, Stage::Stopped) {
            self.stopped.set(true);
        }
        let info = self.info(position_ms, paused, audio, subtitle);
        let api = self.server.api();
        let result = match stage {
            Stage::Start => api.execute(ReportPlaybackStart(info)).await,
            Stage::Progress => api.execute(ReportPlaybackProgress(info)).await,
            Stage::Stopped => api.execute(ReportPlaybackStopped(info)).await,
        };
        if let Err(err) = result {
            debug!("could not report playback ({stage:?}): {err}");
        }
        // Where it was left, and whether it is watched now, show on what was open before it
        // (the page of the item, the rows of the home screen) without opening it again. Only
        // now: the server has it.
        if matches!(stage, Stage::Stopped) {
            let parents = self.parents.iter().copied().map(Some);
            crate::user_data::changed(std::iter::once(Some(self.item_id)).chain(parents));
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.get()
    }
}
