use anyhow::{anyhow, Result};
use uuid::Uuid;

use log::debug;

use super::api::GetMediaSegments;
use super::engine::{StreamKind, SubtitleTrack};
use crate::capabilities::Capabilities;
use crate::media::{Media, MediaSource};
use crate::sdks::{self, MediaSegmentType, MediaStreamType, PlayMethod};
use crate::server::JellyfinServer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    Intro,
    Recap,
    Outro,
    Preview,
    Commercial,
}

impl SegmentKind {
    pub fn skip_label(self) -> &'static str {
        match self {
            SegmentKind::Intro => "Skip intro",
            SegmentKind::Recap => "Skip recap",
            SegmentKind::Outro => "Skip credits",
            SegmentKind::Preview => "Skip preview",
            SegmentKind::Commercial => "Skip ad",
        }
    }
}

/// A stretch of the video the viewer can jump over.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub kind: SegmentKind,
    pub start_ms: f64,
    pub end_ms: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioTrack {
    /// The stream's index in its source.
    pub index: i64,
    pub label: String,
    pub language: Option<String>,
    /// The codec of the stream, as the server names it.
    pub codec: String,
}

/// Everything needed to play one version of an item.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedStream {
    pub item_id: Uuid,
    pub source_id: Uuid,
    pub url: String,
    pub kind: StreamKind,
    pub method: PlayMethod,
    pub play_session_id: Option<String>,
    pub duration_ms: f64,
    pub audio_tracks: Vec<AudioTrack>,
    pub subtitle_tracks: Vec<SubtitleTrack>,
    pub segments: Vec<Segment>,
    /// The audio track the stream plays, and the subtitle to start with.
    pub audio: Option<i64>,
    pub subtitle: Option<i64>,
    /// The file is played as it is and its picture is Dolby Vision: ExoPlayer plays it on the
    /// device's Dolby Vision decoder, tunneled, where it has one (`ExoView.load`).
    pub dolby_vision: bool,
}

/// The answer of the server about how to play `source` of `item`: the version asked for.
async fn playback_source(
    server: &JellyfinServer,
    item: &Media,
    source: Option<&MediaSource>,
    capabilities: &Capabilities,
    dolby_vision: bool,
    embedded_text: bool,
    audio: Option<i64>,
    burned_subtitle: Option<i64>,
) -> Result<(Option<String>, sdks::MediaSourceInfo)> {
    let info = server
        .api()
        .execute(sdks::GetPlaybackInfo {
            item_id: item.id,
            request: sdks::PlaybackInfoRequest {
                user_id: Some(server.user_id()),
                device_profile: Some(capabilities.device_profile(dolby_vision, embedded_text)),
                // Jellyfin compares this as text with the id it has, which has no dashes.
                media_source_id: source.map(|s| s.id.simple().to_string()),
                audio_stream_index: audio.map(|a| a as i32),
                // No engine picks one of the audio tracks of a file itself: another track than the
                // one the file starts with is only had from a stream the server makes of it.
                enable_direct_play: audio.map(|_| false),
                enable_direct_stream: audio.map(|_| false),
                // -1 asks for none: otherwise the server picks one by the user's preference, and
                // burns a picture one in, which means a transcode.
                subtitle_stream_index: Some(burned_subtitle.map_or(-1, |s| s as i32)),
                ..Default::default()
            },
        })
        .await?;
    // The version asked for: the server may list others as well, and in another order from one
    // answer to the next, and the track numbers are those of its own file.
    let mut sources = info.media_sources.into_iter().collect::<Vec<_>>();
    let at = source.and_then(|wanted| sources.iter().position(|s| s.id == wanted.id)).unwrap_or(0);
    if at >= sources.len() {
        return Err(anyhow!("The server returned no media sources"));
    }
    Ok((info.play_session_id, sources.swap_remove(at)))
}

/// Asks the server how to play `item` on this device, and what the version has in it. An
/// `audio` index asks for that track, which can mean a transcode.
pub async fn resolve(
    server: &JellyfinServer,
    item: &Media,
    source: Option<&MediaSource>,
    capabilities: &Capabilities,
    audio: Option<i64>,
    // A picture subtitle to burn into the video.
    burned_subtitle: Option<i64>,
) -> Result<ResolvedStream> {
    debug!(
        "playback info for {} ({:?}): source {:?}, audio {audio:?}, burned subtitle {burned_subtitle:?}",
        item.id,
        item.media_type,
        source.map(|s| s.id),
    );
    let dolby_vision = source.is_some_and(|s| s.dolby_vision_with_fallback());
    // The player reads the text subtitles of a file it plays as it is from the file itself: the
    // server is told so by the profile. A stream the server makes has no such subtitles in it,
    // so then it is asked again, for WebVTT.
    let (mut play_session_id, mut media_source) = playback_source(server, item, source, capabilities, dolby_vision, true, audio, burned_subtitle).await?;
    if media_source.transcoding_url.is_some() {
        (play_session_id, media_source) =
            playback_source(server, item, source, capabilities, dolby_vision, false, audio, burned_subtitle).await?;
    }

    debug!(
        "answer: direct play {}, direct stream {}, transcoding url {}, reasons {:?}; asked for subtitle {:?}: {:?}",
        media_source.supports_direct_play,
        media_source.supports_direct_stream,
        media_source.transcoding_url.is_some(),
        media_source.transcoding_reasons,
        burned_subtitle,
        media_source
            .media_streams
            .iter()
            .find(|s| Some(s.index) == burned_subtitle)
            .map(|s| (s.codec.clone(), s.is_external, s.delivery_method.clone(), s.delivery_url.is_some())),
    );
    let (url, kind, method) = if let Some(transcoding_url) = &media_source.transcoding_url {
        (server.url_for(transcoding_url)?.to_string(), StreamKind::Hls, PlayMethod::Transcode)
    } else if media_source.supports_direct_play || media_source.supports_direct_stream {
        (
            server.direct_stream_url(item.id, media_source.id).to_string(),
            StreamKind::Direct,
            PlayMethod::DirectPlay,
        )
    } else {
        return Err(anyhow!("The server can neither play nor transcode this version"));
    };

    let audio_tracks: Vec<AudioTrack> = media_source
        .media_streams
        .iter()
        .filter(|s| s.type_ == MediaStreamType::Audio)
        .enumerate()
        .map(|(n, s)| AudioTrack {
            index: s.index,
            label: s
                .display_title
                .clone()
                .or_else(|| s.title.clone())
                .or_else(|| s.language.clone())
                .unwrap_or_else(|| format!("Track {}", n + 1)),
            language: s.language.clone(),
            codec: s.codec.clone().unwrap_or_default(),
        })
        .collect();

    // What the player reads or draws from the file itself needs no stream of the server's: a
    // picture subtitle of a codec it draws, a text one that the server delivers embedded (which it
    // does as the profile says), and only of the file as it is. The rest of the text ones come as
    // WebVTT, and the pictures can only be burned into the video.
    let direct = kind == StreamKind::Direct;
    let in_file = |s: &sdks::MediaStream| {
        let codec = s.codec.as_deref().unwrap_or("");
        if s.is_text_subtitle_stream() {
            s.delivery_method == Some(sdks::SubtitleDeliveryMethod::Embed) && capabilities.player_reads_text(codec)
        } else {
            capabilities.player_draws_picture(codec)
        }
    };
    let subtitle_tracks: Vec<SubtitleTrack> = media_source
        .media_streams
        .iter()
        .filter(|s| s.type_ == MediaStreamType::Subtitle)
        // A picture the player draws from the file is not in a stream the server made of it.
        .filter(|s| s.is_text_subtitle_stream() || !in_file(s) || direct)
        .enumerate()
        .map(|(n, s)| {
            // The method looks at the codec; the field of the same name is never read from the server.
            let text = s.is_text_subtitle_stream();
            let in_the_file = direct && in_file(s);
            SubtitleTrack {
                index: s.index,
                label: s
                    .display_title
                    .clone()
                    .or_else(|| s.title.clone())
                    .or_else(|| s.language.clone())
                    .unwrap_or_else(|| format!("Track {}", n + 1)),
                language: s.language.clone(),
                url: if text && !in_the_file {
                    server.subtitle_url(item.id, media_source.id, s.index).to_string()
                } else {
                    String::new()
                },
                burned: !text && !in_file(s),
                codec: s.codec.clone().unwrap_or_default(),
            }
        })
        .collect();

    debug!(
        "{} streams: {} audio, {} subtitles",
        media_source.media_streams.len(),
        audio_tracks.len(),
        subtitle_tracks.len()
    );
    let mut segments = Vec::new();
    // The segments the server knows of the item, which the version doesn't carry.
    // A live channel has no intro or credits.
    let found = if item.is_live() {
        Err(anyhow!("a live channel"))
    } else {
        server.api().execute(GetMediaSegments { item_id: item.id }).await.map_err(Into::into)
    };
    match found {
        Ok(found) => {
            for segment in found.items {
                let kind = match segment.r#type {
                    MediaSegmentType::Intro => SegmentKind::Intro,
                    MediaSegmentType::Outro => SegmentKind::Outro,
                    MediaSegmentType::Recap => SegmentKind::Recap,
                    MediaSegmentType::Preview => SegmentKind::Preview,
                    MediaSegmentType::Commercial => SegmentKind::Commercial,
                    MediaSegmentType::Other => continue,
                };
                segments.push(Segment {
                    kind,
                    start_ms: segment.start_ticks as f64 / 10_000.0,
                    end_ms: segment.end_ticks as f64 / 10_000.0,
                });
            }
        }
        Err(err) => debug!("no segments: {err}"),
    }
    if let Some(found) = &media_source.segments {
        let kinds = [
            (SegmentKind::Recap, &found.recap),
            (SegmentKind::Intro, &found.intro),
            (SegmentKind::Preview, &found.preview),
            (SegmentKind::Outro, &found.outro),
            (SegmentKind::Commercial, &found.commercial),
        ];
        for (kind, segment) in kinds {
            if let Some(segment) = segment {
                if segments.iter().any(|s| s.kind == kind) {
                    continue;
                }
                segments.push(Segment {
                    kind,
                    start_ms: segment.start_ticks as f64 / 10_000.0,
                    end_ms: segment.end_ticks as f64 / 10_000.0,
                });
            }
        }
    }

    // The one asked for, which is burned in, else the version's default if the player can show it.
    let subtitle = burned_subtitle
        .filter(|index| subtitle_tracks.iter().any(|t| t.index == *index && t.burned))
        .or(media_source
            .default_subtitle_stream_index
            .filter(|index| subtitle_tracks.iter().any(|t| t.index == *index && !t.burned)));

    debug!(
        "resolved: source {}, {method:?} as {kind:?}, subtitle {subtitle:?} ({} tracks, {} burned), audio {:?}",
        media_source.id,
        subtitle_tracks.len(),
        subtitle_tracks.iter().filter(|t| t.burned).count(),
        audio.or(media_source.default_audio_stream_index),
    );
    let dolby_vision = matches!(method, PlayMethod::DirectPlay)
        && media_source.media_streams.iter().any(|s| s.type_ == MediaStreamType::Video && s.dv_profile.is_some());
    Ok(ResolvedStream {
        item_id: item.id,
        source_id: media_source.id,
        url,
        kind,
        method,
        play_session_id,
        duration_ms: media_source.run_time_ticks.unwrap_or(0) as f64 / 10_000.0,
        audio: audio.or(media_source.default_audio_stream_index),
        subtitle,
        audio_tracks,
        subtitle_tracks,
        segments,
        dolby_vision,
    })
}
