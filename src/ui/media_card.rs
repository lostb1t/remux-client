//! A card of a film, a show or an episode in a row: its picture, what is marked on it (watched,
//! how far), and for an episode where it is in its show. Ported from Remux's `MediaCard` and
//! `Card`.

use nsh_base::{progress, theme};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;
use nsh_icon::icon;
use nsh_icon_phosphor::Phosphor;

use super::tokens::{is_tv, is_wide, motion, scrim, shade, skeleton, track};
use crate::hooks::server;
use crate::media::{CardVariant, ImageType, Media, MediaType};

/// The size of a card of `shape`: about six posters across a TV, more on a phone (Remux's
/// `image_class`).
pub fn card_size(variant: CardVariant) -> (f32, f32) {
    let landscape = variant == CardVariant::Landscape;
    let square = variant == CardVariant::Square;
    let width = match (landscape, square, is_tv(), is_wide()) {
        (true, _, true, _) => 288.0,
        (true, _, false, _) => 240.0,
        (_, true, true, _) => 208.0,
        (_, true, false, _) => 180.0,
        (_, _, true, _) => 136.0,
        (_, _, false, true) => 160.0,
        (_, _, false, false) => 104.0,
    };
    let height = if landscape {
        width * 9.0 / 16.0
    } else if square {
        width
    } else {
        width * 3.0 / 2.0
    };
    (width, height)
}

/// Where an episode is in its show, "S1 E3 · 60m" (Remux's `episode_label`); a poster is narrow,
/// it has room for where it is, not for how long it is.
fn episode_label(media: &Media, landscape: bool) -> Option<String> {
    if media.media_type != MediaType::Episode {
        return None;
    }
    let place = match (media.season_number, media.index_number) {
        (Some(season), Some(episode)) => Some(format!("S{season} E{episode}")),
        (None, Some(episode)) => Some(format!("E{episode}")),
        _ => None,
    };
    let runtime = media.runtime_seconds.filter(|seconds| *seconds > 0 && landscape).map(|_| media.formatted_runtime());
    let parts: Vec<String> = [place, runtime].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The room between two cards of a row: on a TV, room for a card that grows with the focus and
/// its ring, which may not draw over its neighbours (`z_index` would lift it, but loses the focus).
pub fn card_gap() -> f32 {
    if is_tv() { 16.0 } else { 8.0 }
}

/// How much a card `width` points wide grows with a TV's focus: Remux's 1.12, less for a wide
/// card, so that it stays inside the gap with its ring (4 points, 3 from it).
fn focus_scale(width: f32) -> f32 {
    // Half the gap on each side, less a little: the card does not touch its neighbours.
    let room = card_gap() - 4.0;
    (1.0 + 2.0 * room / width).min(motion::FOCUS_SCALE)
}

/// A round dark badge on a top corner of the picture: the leading one (the watchlist), or the
/// trailing one (watched).
fn badge(glyph: nsh_icon::Icon, leading: bool) -> View {
    let at = view().absolute().top(px(6.0));
    let at = if leading { at.left(px(6.0)) } else { at.right(px(6.0)) };
    at
        .size(px(24.0))
        .rounded_full()
        .items_center()
        .justify_center()
        .bg(scrim())
        .text_color(theme().foreground)
        .child(icon(glyph).size(px(16.0)))
}

/// A card of `media`, in the shape of `variant`. Kept no state of its own: a list gives the
/// cell other items as it is reused, through `media`.
/// `me` names the card (for the remote: a row's first card is where it comes in).
pub fn media_card(me: ElementRef, media: impl Fn() -> Option<Media> + Copy + 'static, variant: CardVariant, on_open: impl Fn(Media) + 'static) -> View {
    card(me, media, variant, false, on_open)
}

/// A card as wide as the column of a grid it is in, with its name and year under it when
/// `caption` (Remux's `fill` and `caption`).
pub fn grid_card(media: impl Fn() -> Option<Media> + Copy + 'static, variant: CardVariant, caption: bool, on_open: impl Fn(Media) + 'static) -> View {
    let card = card(element_ref(), media, variant, true, on_open);
    if caption { captioned(card, media) } else { card }
}

/// A card of a row with its name and year under it, as wide as the card (Remux's `caption`, which
/// music rows have).
pub fn captioned_card(me: ElementRef, media: impl Fn() -> Option<Media> + Copy + 'static, variant: CardVariant, on_open: impl Fn(Media) + 'static) -> View {
    let (width, _) = card_size(variant);
    captioned(card(me, media, variant, false, on_open), media).w(px(width))
}

/// `card` with the name and the year of `media` under it.
fn captioned(card: View, media: impl Fn() -> Option<Media> + Copy + 'static) -> View {
    use chrono::Datelike;
    view()
        .gap(px(2.0))
        .child(card)
        .child(text(move || media().map(|media| SharedString::from(media.title)).unwrap_or_default()).mt(px(4.0)).text_sm().font_weight(FontWeight::MEDIUM).number_of_lines(1).text_color(theme().foreground))
        .child(text(move || media().and_then(|media| media.release_date).map(|date| SharedString::from(date.year().to_string())).unwrap_or_default()).text_xs().number_of_lines(1).text_color(theme().muted_foreground))
}

fn card(me: ElementRef, media: impl Fn() -> Option<Media> + Copy + 'static, variant: CardVariant, fill: bool, on_open: impl Fn(Media) + 'static) -> View {
    let (width, height) = card_size(variant);
    let landscape = variant == CardVariant::Landscape;
    let kind = if landscape { ImageType::Thumb } else { ImageType::Poster };
    // At most the pixels it is shown with (about three per point on a phone).
    let pixels = (width * 3.0) as u32;
    let picture = move || {
        media().and_then(|media| {
            let server = server();
            server.image_url_at(&media, kind, pixels).or_else(|| server.image_url_at(&media, ImageType::Backdrop, pixels))
        })
    };
    // Without a picture of its own kind (a poster, a thumb), its title on the card: on the
    // backdrop it falls back to, or on nothing at all (Remux's fallback).
    let untitled = move || media().and_then(|media| server().image_url_at(&media, kind, pixels).is_none().then_some(media.title));
    let label = move || media().and_then(|media| episode_label(&media, landscape));
    let progress_value = move || media().and_then(|media| media.progress()).map(|progress| progress as f32);

    // A view with a button's role, not `button`: a card also has a long press (its menu), which a
    // view has and `button` has not.
    view()
        .element_ref(&me)
        .focusable(true)
        .role(AccessibilityRole::Button)
        .when(!fill, |v| v.w(px(width)).h(px(height)))
        .when(fill, |v| v.w_full().aspect_ratio(width / height))
        .rounded_lg()
        .overflow_hidden()
        .bg(skeleton())
        .label(move || media().map(|media| SharedString::from(media.title)).unwrap_or_default())
        .on_press(move || {
            if let Some(media) = media() {
                // Back comes back to this card.
                crate::tv_focus::pressed(me);
                on_open(media);
            }
        })
        // Held: what can be done with it (on a remote, OK held).
        .on_long_press(move |_: &PressEvent| {
            if let Some(media) = media() {
                crate::components::open_item_menu(media);
            }
        })
        .transition_transform()
        .duration(motion::FAST)
        .ease_out()
        .active(|s| s.scale(motion::PRESS_SCALE))
        // The remote's focus: the poster grows with a ring around it (Remux: `scale-[1.12]`). No
        // `z_index` to lift it over its neighbours: a change of it reorders the native views, and
        // the card lost the focus it had just been given (Android TV, NSH 0.7.0).
        // On a TV the card only grows, with a shadow under it: a ring around a rounded picture is a
        // square on Android, a white frame. Elsewhere (a keyboard) the ring shows where it is.
        .focus_visible(move |s| {
            if is_tv() {
                s.scale(focus_scale(width)).shadow_lg()
            } else {
                s.outline(px(3.0)).outline_offset(px(3.0)).outline_color(theme().ring)
            }
        })
        .child(
            image(move || ImageSrc::from(picture().unwrap_or_default()))
                .absolute()
                .inset_0()
                .object_fit(ResizeMode::Cover)
                .transition_opacity()
                .duration(motion::BASE),
        )
        .child(when(
            move || untitled().is_some(),
            move || {
                view()
                    .absolute()
                    .inset_0()
                    .p_3()
                    .justify_center()
                    .items_center()
                    .bg(scrim())
                    .child(
                        text(move || SharedString::from(untitled().unwrap_or_default()))
                            .text_center()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .number_of_lines(4)
                            .text_color(theme().foreground),
                    )
            },
        ))
        .child(when(move || media().is_some_and(|media| media.is_watchlist()), || badge(Phosphor::BookmarkSimpleFill, true)))
        .child(when(move || media().is_some_and(|media| media.is_watched()), || badge(Phosphor::CheckCircleFill, false)))
        .child(when(
            move || label().is_some(),
            move || {
                view()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(64.0))
                    .justify_end()
                    .px_3()
                    .pb(px(6.0))
                    .bg_linear_to_t()
                    .gradient_from(shade())
                    .gradient_to(theme().background.opacity(0.0))
                    .child(
                        text(move || SharedString::from(label().unwrap_or_default()))
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme().foreground)
                            .number_of_lines(1),
                    )
            },
        ))
        .child(when(
            move || progress_value().is_some(),
            move || {
                progress()
                    .value(move || progress_value().unwrap_or_default())
                    .accessibility_label("Watched")
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .track_style(|s| s.h(px(4.0)).bg(track()))
                    .indicator_style(|s| s.h_full().bg(theme().primary))
            },
        ))
}
