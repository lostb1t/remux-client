//! The app's tokens beyond the theme's standard roles (`theme.json`), its motion, and the kind of
//! screen it is on. Components read these; features do not write colors, durations or sizes.

use nsh_base::theme;
use nsh_core::platform::Idiom;
use nsh_core::prelude::*;

/// A custom color of `theme.json`. A misspelled name shows as the destructive color, so that it is
/// seen in review rather than missed.
fn color(name: &str) -> AdaptiveColor {
    theme().color(name).unwrap_or(theme().destructive)
}

/// Frosted glass of the bars that float over the page.
pub fn glass() -> AdaptiveColor {
    // PERFORMANCE: a TV's chip has no blur to spare (as in Remux): a solid surface there.
    if is_tv() { color("glass-solid") } else { color("glass") }
}
/// The light line at the top edge of glass.
pub fn glass_highlight() -> AdaptiveColor {
    color("glass-highlight")
}
/// The dark round behind a badge on a picture.
pub fn scrim() -> AdaptiveColor {
    color("scrim")
}
/// The shade at the bottom of a picture, under its label.
pub fn shade() -> AdaptiveColor {
    color("shade")
}
/// Text that is less than the main text, on a picture or the page.
pub fn secondary_text() -> AdaptiveColor {
    color("secondary-text")
}
/// The audience's star.
pub fn rating_star() -> AdaptiveColor {
    color("rating-star")
}
/// What stands in for something that is still loading.
pub fn skeleton() -> AdaptiveColor {
    color("skeleton")
}
/// The panel of a sheet.
pub fn sheet() -> AdaptiveColor {
    color("sheet")
}
/// A group of rows in a sheet or a settings page.
pub fn group() -> AdaptiveColor {
    color("group")
}
/// What confirms: a check in a field.
pub fn positive() -> AdaptiveColor {
    color("positive")
}
/// The tint of the Favorites banner.
pub fn favorites_tint() -> AdaptiveColor {
    color("favorites-tint")
}
/// The tint of the Playlists banner.
pub fn playlists_tint() -> AdaptiveColor {
    color("playlists-tint")
}
/// What is on now: a programme's progress.
pub fn live() -> AdaptiveColor {
    color("live")
}
/// The round of a person without a picture.
pub fn avatar() -> AdaptiveColor {
    color("avatar")
}
/// A subtitle's text over the video, and the edge around its letters (Remux: white with a black
/// outline, readable on any picture without a box hiding it).
pub fn subtitle() -> AdaptiveColor {
    color("subtitle")
}
pub fn subtitle_edge() -> AdaptiveColor {
    color("subtitle-edge")
}
/// The track of a progress bar.
pub fn track() -> AdaptiveColor {
    color("track")
}

/// Motion, in milliseconds (the Design Guides' starting set).
pub mod motion {
    /// A hover wash, a color change.
    pub const INSTANT: f32 = 120.0;
    /// A press, a control's state, a poster growing with the focus.
    pub const FAST: f32 = 180.0;
    /// Moving into place: a picture fading in.
    pub const BASE: f32 = 240.0;
    /// Reveals: a sheet coming in.
    pub const SLOW: f32 = 360.0;
    /// A press.
    pub const PRESS_SCALE: f32 = 0.97;
    /// An icon button's press.
    pub const ICON_PRESS_SCALE: f32 = 0.95;
    /// A poster with the remote's focus: seen from a couch (Remux: `scale-[1.12]`).
    pub const FOCUS_SCALE: f32 = 1.12;
}

/// A TV: seen from a distance, driven with a remote, often with a weak chip.
pub fn is_tv() -> bool {
    idiom() == Idiom::Tv
}

/// A wide screen (Tailwind's `lg`, 1024 points), or a TV (960 points across at its density, as
/// Remux's `data-tv` pages): the hero is a banner, the text left aligned. Reactive: it follows
/// the window.
pub fn is_wide() -> bool {
    is_tv() || window().width.0 >= 1024.0
}

/// Whether the places are in the menu at the top left (the TV's: a pill that opens a panel), not
/// in a tab bar at the bottom: a TV, and a wide window of a desktop or a browser. A phone keeps its
/// tab bar, also turned on its side.
pub fn has_menu() -> bool {
    is_tv() || (!cfg!(any(target_os = "android", target_os = "ios")) && is_wide())
}

/// The room under a page's last row, so the tab bar does not cover it (`base`), and the music bar
/// above it when something plays. Read in a binding: it grows when the music starts.
pub fn bottom_room(base: f32) -> impl Fn() -> Points + Copy + 'static {
    move || {
        let music = !is_tv() && crate::music::queue().with(|queue| queue.index.is_some());
        px(base + if music { 76.0 } else { 0.0 })
    }
}
