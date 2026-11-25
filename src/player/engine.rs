//! What the player plays, as the server gives it (Remux's `player/engine.rs`, the types; the
//! engine itself is NSH's `video` element, `pages/player.rs`).

/// How a stream reaches the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamKind {
    /// The file itself, as one request.
    Direct,
    /// An HLS playlist the server transcodes into.
    Hls,
}

/// A subtitle of a version: a text one the server delivers as WebVTT, or a picture one (such as
/// PGS), which the server has to burn into the video, so that picking it asks for another stream.
#[derive(Clone, Debug, PartialEq)]
pub struct SubtitleTrack {
    /// The stream's index in its source.
    pub index: i64,
    pub label: String,
    pub language: Option<String>,
    /// Where the WebVTT is; empty for a burned one, and for a picture one the player draws from
    /// the file itself.
    pub url: String,
    pub burned: bool,
    /// The codec of the stream, as the server names it.
    pub codec: String,
}
