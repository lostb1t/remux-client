use crate::sdks::{
    self,
    codecs::{CodecProfileType, DlnaProfileType, TranscodingProtocol},
    ProfileConditionProperty, ProfileConditionType,
};
use log::debug;

/// Set when a device says it decodes Dolby audio and then fails on it (some phones do): from then
/// on the server is asked to turn that audio into something else.
pub static DOLBY_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Devices whose Dolby decoder says yes and then fails: the Pixels and everything with a Google
/// Tensor chip. They are not asked to play AC-3 and E-AC-3 as it is, the server turns it into
/// something else. Others that fail the same are found out by `DOLBY_FAILED`.
fn known_bad_dolby_decoder() -> bool {
    #[cfg(target_os = "android")]
    {
        static BAD: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *BAD.get_or_init(|| {
            let prop = |name: &str| {
                std::process::Command::new("getprop")
                    .arg(name)
                    .output()
                    .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_lowercase())
                    .unwrap_or_default()
            };
            let pixel = (prop("ro.product.manufacturer") == "google" || prop("ro.product.brand") == "google")
                && prop("ro.product.model").contains("pixel");
            // Only Android 12 and later say which chip it is.
            let soc = prop("ro.soc.model");
            let bad = pixel || soc.starts_with("tensor") || soc.starts_with("gs");
            if bad {
                log::info!("Dolby audio is left to the server on this device");
            }
            bad
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// What the browser can play, as reported by [`Capabilities::detect_browser_capabilities`].
#[derive(Clone, Debug, PartialEq)]
pub struct Capabilities {
    pub supports_h264: bool,
    pub supports_vp9: bool,
    pub supports_av1: bool,
    pub supports_hevc: bool,
    pub supports_ac3: bool,
    pub supports_eac3: bool,
    pub supports_flac: bool,
    pub supports_aac: bool,
    pub supports_opus: bool,
    pub supports_vorbis: bool,
    pub supports_mp3: bool,
    /// Matroska files, which browsers read when the codecs inside are ones they know.
    pub supports_mkv: bool,
    /// A display that shows HDR.
    pub supports_hdr: bool,
    /// mpv, which decodes nearly everything itself and mixes the sound down: the profile is wide.
    pub native: bool,
    /// The largest picture the decoder takes, as width and height, where the device says. The
    /// server transcodes what is larger.
    pub h264_max: Option<(u32, u32)>,
    pub hevc_max: Option<(u32, u32)>,
    pub user_agent: String,
}

impl Capabilities {
    /// Whether the player draws a picture subtitle of this codec (PGS and the like) from the file
    /// itself, so that the server has no need to burn it into the video. mpv draws all of them;
    /// ExoPlayer, PGS and DVB; a browser none.
    pub fn player_draws_picture(&self, codec: &str) -> bool {
        let codec = codec.to_lowercase();
        if !self.native {
            return false;
        }
        if cfg!(any(target_os = "ios", target_os = "macos", windows_mpv)) {
            return true;
        }
        cfg!(target_os = "android")
            && matches!(codec.as_str(), "pgs" | "pgssub" | "hdmv_pgs_subtitle" | "dvbsub" | "dvb_subtitle")
    }

    /// Whether the player reads a text subtitle of this codec from the file itself, so that the
    /// server need not make a WebVTT of it: mpv reads all of them, ExoPlayer the common ones, a
    /// browser none (it cannot see inside a Matroska file).
    pub fn player_reads_text(&self, codec: &str) -> bool {
        if !self.native {
            return false;
        }
        if cfg!(any(target_os = "ios", target_os = "macos", windows_mpv)) {
            return true;
        }
        cfg!(target_os = "android")
            && matches!(
                codec.to_lowercase().as_str(),
                "srt" | "subrip" | "ass" | "ssa" | "vtt" | "webvtt" | "mov_text"
            )
    }

    /// Whether the player picks an audio track of this codec of the file itself, as mpv does for
    /// all of them and ExoPlayer for what the device plays. For the rest (and for a browser) the
    /// server makes a stream that has the track.
    pub fn player_picks_audio(&self, codec: &str) -> bool {
        if !self.native {
            return false;
        }
        if cfg!(any(target_os = "ios", target_os = "macos", windows_mpv)) {
            return true;
        }
        let codec = codec.to_lowercase();
        cfg!(target_os = "android")
            && match codec.as_str() {
                "aac" | "mp3" | "opus" | "flac" | "vorbis" => true,
                "ac3" => self.supports_ac3,
                "eac3" => self.supports_eac3,
                _ => false,
            }
    }

    /// What the player plays: ExoPlayer with the device's decoders (Remux's profile for it where
    /// the device could not be asked: wide). HDR is left to the server to tone map until the
    /// screen is asked (PLAN.md).
    fn nsh() -> Self {
        let hdr = display_hdr();
        Self {
            supports_h264: true,
            supports_vp9: true,
            supports_av1: true,
            supports_hevc: true,
            supports_ac3: true,
            supports_eac3: true,
            supports_flac: true,
            supports_aac: true,
            supports_opus: true,
            supports_vorbis: true,
            supports_mp3: true,
            supports_mkv: true,
            // The picture as it is where the screen shows HDR, unless it is turned off in the
            // settings; else the server makes it SDR.
            supports_hdr: hdr.screen_shows_hdr() && !crate::settings::settings().hdr_off,
            native: true,
            h264_max: None,
            hevc_max: None,
            user_agent: "Remux".to_string(),
        }
    }

    /// What a Chromecast plays, by what it says it is: every model takes H.264 in an mp4 and
    /// the usual audio, and the newer ones (Ultra, Google TV, TVs with it built in) HEVC and VP9
    /// as well. DTS and TrueHD, Matroska and AV1 are for the server to transcode.
    pub fn chromecast(model: &str) -> Self {
        let newer = model.contains("Ultra") || model.contains("Google") || model.to_lowercase().contains("tv");
        Self {
            supports_h264: true,
            supports_vp9: newer,
            supports_av1: false,
            supports_hevc: newer,
            supports_ac3: true,
            supports_eac3: true,
            supports_flac: true,
            supports_aac: true,
            supports_opus: true,
            supports_vorbis: false,
            supports_mp3: true,
            supports_mkv: false,
            supports_hdr: false,
            native: false,
            h264_max: None,
            hevc_max: None,
            user_agent: "Remux".to_string(),
        }
    }

    /// What this device plays.
    /// What this device plays: ExoPlayer's wide profile in the apps; in a browser, what the
    /// browser says it plays (Remux's web detection), so the server converts the rest.
    pub fn detect() -> Self {
        #[cfg(target_arch = "wasm32")]
        if let Some(browser) = web::detect() {
            return browser;
        }
        Self::nsh()
    }

    /// The profile the server decides on: what to play as it is, what to transcode into, and
    /// what the browser can't take (an HDR picture, a level its decoder lacks).
    pub fn to_device_profile(&self) -> sdks::DeviceProfile {
        self.device_profile(false, true)
    }

    /// The profile for one version: `dolby_vision` when it is Dolby Vision with an HDR10 layer
    /// under it. This server calls all Dolby Vision "DOVI", so it is on the version that the
    /// profile can tell whether it may be played as it is.
    ///
    /// `embedded_text` lets the player read the text subtitles of the file itself (as it can, for
    /// a file it plays as it is); without it the server makes a WebVTT of each, which is what a
    /// stream the server makes (a transcode) has to have.
    pub fn device_profile(&self, dolby_vision: bool, embedded_text: bool) -> sdks::DeviceProfile {
        debug!("{:?}", &self);

        let mut video: Vec<&str> = Vec::new();
        if self.supports_h264 {
            video.push("h264");
        }
        if self.supports_hevc {
            video.push("hevc");
        }
        if self.supports_av1 {
            video.push("av1");
        }
        if self.supports_vp9 {
            video.push("vp9");
        }

        // Aac and mp3 every browser plays; the rest as detected.
        let mut audio: Vec<&str> = vec!["aac", "mp3"];
        let dolby = !DOLBY_FAILED.load(std::sync::atomic::Ordering::Relaxed) && !known_bad_dolby_decoder();
        let optional = [
            (self.supports_opus, "opus"),
            (self.supports_flac, "flac"),
            (self.supports_ac3 && dolby, "ac3"),
            (self.supports_eac3 && dolby, "eac3"),
        ];
        audio.extend(optional.iter().filter(|(on, _)| *on).map(|(_, codec)| *codec));
        // What the native players decode besides: the lossless and DTS tracks of discs. Not
        // ExoPlayer, which has only what the device decodes.
        if self.native && cfg!(not(target_os = "android")) {
            audio.extend(["truehd", "dts", "alac", "pcm_s16le", "pcm_s24le"]);
        }

        let mut direct_play_profiles = Vec::new();
        if !video.is_empty() {
            direct_play_profiles.push(video_direct_play(&["mp4", "m4v", "mov"], &video, &audio));
        }
        let webm_video: Vec<&str> = ["vp9", "av1"]
            .into_iter()
            .filter(|codec| video.contains(codec))
            .collect();
        if !webm_video.is_empty() {
            let mut webm_video = webm_video;
            webm_video.insert(0, "vp8");
            direct_play_profiles.push(video_direct_play(&["webm"], &webm_video, &["opus", "vorbis"]));
        }
        if self.supports_mkv && !video.is_empty() {
            let mut mkv_audio = audio.clone();
            if self.supports_vorbis {
                mkv_audio.push("vorbis");
            }
            direct_play_profiles.push(video_direct_play(&["mkv"], &video, &mkv_audio));
        }
        let audio_only = [
            (self.supports_mp3, "mp3", "mp3"),
            (self.supports_aac, "aac", "aac"),
            (self.supports_aac, "m4a", "aac"),
            (self.supports_flac, "flac", "flac"),
            (self.supports_opus, "opus", "opus"),
            (self.supports_vorbis, "ogg", "vorbis"),
        ];
        for (supported, container, codec) in audio_only {
            if supported {
                direct_play_profiles.push(sdks::DirectPlayProfile {
                    container: parse_all(&[container]),
                    audio_codec: parse_all(&[codec]),
                    video_codec: None,
                    type_: Some(DlnaProfileType::Audio),
                });
            }
        }

        let transcoding_profiles = vec![
            sdks::TranscodingProfile {
                container: "ts".parse().ok(),
                type_: Some(DlnaProfileType::Video),
                video_codec: parse_all(&["h264"]),
                audio_codec: parse_all(&["aac"]),
                protocol: Some(TranscodingProtocol::Hls),
                ..Default::default()
            },
            // What a track that the device cannot play is made into: music, as AAC in HLS.
            sdks::TranscodingProfile {
                container: "ts".parse().ok(),
                type_: Some(DlnaProfileType::Audio),
                video_codec: None,
                audio_codec: parse_all(&["aac"]),
                protocol: Some(TranscodingProtocol::Hls),
                ..Default::default()
            },
        ];

        // What browsers' decoders take. Without these the server would hand over a picture
        // the browser can't show, such as 10-bit H.264 or HDR on a screen without it.
        // The size the decoder takes, where the device says: the server then transcodes what is
        // larger, and ranks the versions by it (it wants both, width and height, to rank).
        let size = |max: Option<(u32, u32)>| -> Vec<sdks::ProfileCondition> {
            max.map(|(width, height)| {
                vec![
                    condition(ProfileConditionType::LessThanEqual, ProfileConditionProperty::Width, &width.to_string()),
                    condition(ProfileConditionType::LessThanEqual, ProfileConditionProperty::Height, &height.to_string()),
                ]
            })
            .unwrap_or_default()
        };
        let mut h264 = vec![
            condition(ProfileConditionType::EqualsAny, ProfileConditionProperty::VideoProfile, "high|main|baseline|constrained baseline"),
            condition(ProfileConditionType::LessThanEqual, ProfileConditionProperty::VideoLevel, "52"),
            condition(ProfileConditionType::LessThanEqual, ProfileConditionProperty::VideoBitDepth, "8"),
        ];
        h264.extend(size(self.h264_max));
        let mut codec_profiles = vec![video_profile("h264", &h264)];
        if self.supports_hevc {
            // A decoder without HEVC Main 10 plays 8 bits only.
            let ten_bit = !(cfg!(target_os = "android") && self.native) ;
            let mut hevc = vec![
                condition(
                    ProfileConditionType::EqualsAny,
                    ProfileConditionProperty::VideoProfile,
                    if ten_bit { "main|main 10" } else { "main" },
                ),
                condition(ProfileConditionType::LessThanEqual, ProfileConditionProperty::VideoBitDepth, if ten_bit { "10" } else { "8" }),
            ];
            hevc.extend(size(self.hevc_max));
            codec_profiles.push(video_profile("hevc", &hevc));
        }
        // With HDR on, the picture goes to the screen as it is. Dolby Vision files that have an
        // HDR10 (or HLG) layer to fall back on play as that, which a screen without Dolby Vision
        // shows as well. A file that has only Dolby Vision (profile 5) needs the decoder for it,
        // which the player does not use, so that is left to the server.
        let ranges = match (self.supports_hdr, dolby_vision) {
            (false, _) => "SDR",
            (true, false) => "SDR|HDR10|HLG|HDR10Plus|DOVIWithHDR10|DOVIWithHDR10Plus|DOVIWithHLG|DOVIWithSDR|DOVIWithEL|DOVIWithELHDR10Plus",
            (true, true) => "SDR|HDR10|HLG|HDR10Plus|DOVIWithHDR10|DOVIWithHDR10Plus|DOVIWithHLG|DOVIWithSDR|DOVIWithEL|DOVIWithELHDR10Plus|DOVI",
        };
        codec_profiles.push(sdks::CodecProfile {
            type_: Some(CodecProfileType::Video),
            conditions: vec![condition(
                ProfileConditionType::EqualsAny,
                ProfileConditionProperty::VideoRangeType,
                ranges,
            )],
            ..Default::default()
        });
        codec_profiles.push(sdks::CodecProfile {
            type_: Some(CodecProfileType::VideoAudio),
            conditions: vec![condition(
                ProfileConditionType::LessThanEqual,
                ProfileConditionProperty::AudioChannels,
                // A player that mixes down takes all of them.
                if self.native { "8" } else { "6" },
            )],
            ..Default::default()
        });

        sdks::DeviceProfile {
            name: Some(self.user_agent.clone()),
            // A browser on a good connection plays what the server has; the viewer can ask for
            // less. Too low a limit turns every large remux into a transcode.
            max_streaming_bitrate: Some(120_000_000),
            max_static_bitrate: Some(120_000_000),
            direct_play_profiles,
            subtitle_profiles: self.subtitle_profiles(embedded_text),
            transcoding_profiles,
            codec_profiles,
            ..Default::default()
        }
    }
}

fn parse_all<T: std::str::FromStr>(items: &[&str]) -> Option<Vec<T>> {
    let parsed: Vec<T> = items.iter().filter_map(|item| item.parse().ok()).collect();
    (!parsed.is_empty()).then_some(parsed)
}

fn video_direct_play(containers: &[&str], video: &[&str], audio: &[&str]) -> sdks::DirectPlayProfile {
    sdks::DirectPlayProfile {
        container: parse_all(containers),
        video_codec: parse_all(video),
        audio_codec: parse_all(audio),
        type_: Some(DlnaProfileType::Video),
    }
}

fn condition(
    condition: ProfileConditionType,
    property: ProfileConditionProperty,
    value: &str,
) -> sdks::ProfileCondition {
    sdks::ProfileCondition {
        condition: Some(condition),
        property: Some(property),
        value: Some(value.to_string()),
        is_required: Some(false),
    }
}

fn video_profile(codec: &str, conditions: &[sdks::ProfileCondition]) -> sdks::CodecProfile {
    sdks::CodecProfile {
        type_: Some(CodecProfileType::Video),
        codec: Some(vec![codec.to_string()]),
        conditions: conditions.to_vec(),
        ..Default::default()
    }
}

impl Capabilities {
    /// Text subtitles the player reads from the file itself are in the file (`embedded_text`); the
    /// others come as WebVTT, which the server makes. Picture ones the player draws itself are in
    /// the file too, and the rest are burned into the video by the server.
    fn subtitle_profiles(&self, embedded_text: bool) -> Vec<sdks::SubtitleProfile> {
        let text = [
            "vtt", "webvtt", "srt", "subrip", "ass", "ssa", "mov_text", "ttml", "microdvd", "mpl2",
            "pjs", "smi", "subviewer", "text", "vplayer",
        ];
        let pictures = ["pgs", "pgssub", "dvdsub", "dvbsub", "idx", "xsub"];
        text.iter()
            .map(|format| {
                let method = if embedded_text && self.player_reads_text(format) {
                    sdks::SubtitleDeliveryMethod::Embed
                } else {
                    sdks::SubtitleDeliveryMethod::External
                };
                (*format, method)
            })
            .chain(pictures.iter().map(|format| {
                let method = if self.player_draws_picture(format) {
                    sdks::SubtitleDeliveryMethod::Embed
                } else {
                    sdks::SubtitleDeliveryMethod::Encode
                };
                (*format, method)
            }))
            .map(|(format, method)| sdks::SubtitleProfile {
                format: Some(format.to_string()),
                method: Some(method),
            })
            .collect()
    }
}



/// What the device says about HDR (`DeviceInfo.hdrBits`), read once as the app starts; -1 until
/// then (and where there is no such thing to ask).
static DISPLAY_HDR: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

pub fn set_display_hdr(bits: i32) {
    log::info!("display HDR bits: {bits}");
    DISPLAY_HDR.store(bits, std::sync::atomic::Ordering::Relaxed);
}

/// What the screen and the decoder do with HDR.
#[derive(Clone, Copy, Debug, Default)]
pub struct DisplayHdr {
    pub known: bool,
    pub dolby_vision: bool,
    pub hdr10: bool,
    pub hlg: bool,
    pub hdr10_plus: bool,
    /// Android calls the screen HDR.
    pub screen_hdr: bool,
    pub hevc_main10: bool,
}

impl DisplayHdr {
    pub fn screen_shows_hdr(&self) -> bool {
        self.dolby_vision || self.hdr10 || self.hlg || self.hdr10_plus || self.screen_hdr
    }

    /// For the settings, to see what a device says.
    pub fn describe(&self) -> String {
        if !self.known {
            return "Screen: not known".to_owned();
        }
        let formats: Vec<&str> = [(self.dolby_vision, "Dolby Vision"), (self.hdr10, "HDR10"), (self.hdr10_plus, "HDR10+"), (self.hlg, "HLG")]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, name)| name)
            .collect();
        match (formats.is_empty(), self.screen_hdr) {
            (true, false) => "Screen: SDR".to_owned(),
            (true, true) => "Screen: HDR".to_owned(),
            (false, _) => format!("Screen: {}", formats.join(", ")),
        }
    }
}

pub fn display_hdr() -> DisplayHdr {
    let bits = DISPLAY_HDR.load(std::sync::atomic::Ordering::Relaxed);
    if bits < 0 {
        return DisplayHdr::default();
    }
    DisplayHdr {
        known: true,
        dolby_vision: bits & 1 != 0,
        hdr10: bits & 2 != 0,
        hlg: bits & 4 != 0,
        hdr10_plus: bits & 8 != 0,
        screen_hdr: bits & 16 != 0,
        hevc_main10: bits & 32 != 0,
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::JsCast;
    use web_sys::HtmlMediaElement;

    use super::Capabilities;

    const HEVC: &[&str] = &[
        r#"video/mp4; codecs="hvc1.1.L120""#,
        r#"video/mp4; codecs="hev1.1.L120""#,
        r#"video/mp4; codecs="hvc1.1.0.L120""#,
        r#"video/mp4; codecs="hev1.1.0.L120""#,
    ];
    const AV1: &[&str] = &[
        r#"video/mp4; codecs="av01.0.05M.08""#,
        r#"video/mp4; codecs="av01.0.08M.08""#,
    ];
    const H264: &[&str] = &[
        r#"video/mp4; codecs="avc1.42E01E, mp4a.40.2""#,
        r#"video/mp4; codecs="avc1.4d401e""#,
        r#"video/mp4; codecs="avc1.640028""#,
    ];
    const VP9: &[&str] = &[r#"video/webm; codecs="vp9, opus""#, r#"video/webm; codecs="vp9""#];
    const EAC3: &[&str] = &[r#"audio/mp4; codecs="ec-3""#, "audio/eac3"];
    const FLAC: &[&str] = &["audio/flac", "audio/x-flac", r#"audio/webm; codecs="flac""#];
    const AAC: &[&str] = &[r#"audio/mp4; codecs="mp4a.40.2""#, "audio/aac"];
    const OPUS: &[&str] = &[r#"audio/webm; codecs="opus""#, r#"audio/ogg; codecs="opus""#];
    const MP3: &[&str] = &["audio/mpeg", "audio/mp3"];
    const AC3: &[&str] = &[r#"audio/mp4; codecs="ac-3""#, "audio/ac3"];
    const VORBIS: &[&str] = &[r#"audio/webm; codecs="vorbis""#, r#"audio/ogg; codecs="vorbis""#];
    const MKV: &[&str] = &[
        r#"video/x-matroska; codecs="avc1.640028, mp4a.40.2""#,
        "video/x-matroska",
    ];

    pub fn detect() -> Option<Capabilities> {
        let window = web_sys::window()?;
        let video: HtmlMediaElement = window
            .document()?
            .create_element("video")
            .ok()?
            .dyn_into()
            .ok()?;
        let can_play = |types: &[&str]| {
            types
                .iter()
                .any(|kind| !video.can_play_type(kind).is_empty())
        };
        let hdr = window
            .match_media("(dynamic-range: high)")
            .ok()
            .flatten()
            .is_some_and(|query| query.matches());

        Some(Capabilities {
            supports_h264: can_play(H264),
            supports_vp9: can_play(VP9),
            supports_av1: can_play(AV1),
            supports_hevc: can_play(HEVC),
            supports_ac3: can_play(AC3),
            supports_eac3: can_play(EAC3),
            supports_flac: can_play(FLAC),
            supports_aac: can_play(AAC),
            supports_opus: can_play(OPUS),
            supports_vorbis: can_play(VORBIS),
            supports_mp3: can_play(MP3),
            supports_mkv: can_play(MKV),
            supports_hdr: hdr,
            native: false,
            h264_max: None,
            hevc_max: None,
            user_agent: window.navigator().user_agent().unwrap_or_default(),
        })
    }
}
