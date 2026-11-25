//! Buttons: the app's look over `nsh_base::button`, which brings the role, focus and keys.

use nsh_base::{button, theme};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;
use nsh_icon::{icon, labeled_icon};

use super::tokens::{is_tv, motion};

/// What a button is for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// The one action of a surface: white, dark text (Remux's Play).
    Primary,
    /// The others: a dark round.
    Secondary,
}

/// How big it is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// A phone's.
    Regular,
    /// Big enough to aim at from a couch, or on a wide screen.
    Large,
}

impl Size {
    fn height(self) -> f32 {
        match self {
            Size::Regular => 40.0,
            Size::Large => 44.0,
        }
    }
}

/// The colors, press, focus ring and motion every button shares.
fn base(variant: Variant, size: Size) -> nsh_base::ButtonElement {
    let (bg, fg) = match variant {
        Variant::Primary => (theme().primary, theme().primary_foreground),
        Variant::Secondary => (theme().secondary, theme().secondary_foreground),
    };
    button()
        .h(px(size.height()))
        .flex_row()
        .items_center()
        .justify_center()
        .gap_2()
        .rounded_full()
        .bg(bg)
        .text_color(fg)
        .transition_transform()
        .duration(motion::FAST)
        .ease_out()
        .active(|s| s.scale(motion::PRESS_SCALE).opacity(0.9))
        .hover(|s| s.opacity(0.9))
        // A remote's focus: the button turns white and grows, as in Remux.
        .focus_visible(move |s| {
            s.scale(if is_tv() { 1.05 } else { 1.0 })
                .bg(theme().primary)
                .text_color(theme().primary_foreground)
                .outline(px(3.0))
                .outline_offset(px(3.0))
                .outline_color(theme().ring)
        })
        .disabled_style(|s| s.opacity(0.5))
}

/// A button with an icon and a label.
pub fn text_button(
    variant: Variant,
    size: Size,
    glyph: impl IntoReactive<ImageSrc>,
    label: impl Into<SharedString>,
    on_press: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    let large = size == Size::Large;
    let label: SharedString = label.into();
    base(variant, size)
        .accessibility_label(label.clone())
        .px_6()
        .min_w(px(if large { 256.0 } else { 160.0 }))
        .on_press(on_press)
        .child(icon(glyph).size(px(18.0)))
        .child(text(label).font_weight(FontWeight::SEMIBOLD).when(large, |t| t.text_base()).when(!large, |t| t.text_sm()))
}

/// A round button with only an icon, which says what it does to screen readers.
pub fn icon_button(
    size: Size,
    glyph: impl IntoReactive<ImageSrc>,
    label: impl Into<SharedString>,
    on_press: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    let label: SharedString = label.into();
    base(Variant::Secondary, size)
        .w(px(size.height()))
        .active(|s| s.scale(motion::ICON_PRESS_SCALE).opacity(0.9))
        .accessibility_label(label.clone())
        .on_press(on_press)
        .child(labeled_icon(glyph, label).size(px(19.0)))
}
