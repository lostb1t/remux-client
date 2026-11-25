//! What a place shows while it waits, or when it has nothing (Remux's spinner and empty texts).

use nsh_base::theme;
use nsh_core::prelude::*;

/// The system's spinner while something loads (Android's own, `spinner_android.rs`).
#[cfg(target_os = "android")]
pub fn spinner() -> View {
    view().size(px(32.0)).label("Loading").child(native(super::spinner_android::NAME).size_full())
}

/// A ring that turns while something loads; still with reduced motion.
#[cfg(not(target_os = "android"))]
pub fn spinner() -> View {
    view()
        .size(px(32.0))
        .rounded_full()
        .border(px(4.0))
        .border_color(theme().foreground.opacity(0.2))
        .border_t_color(theme().foreground)
        .when(!reduce_motion(), |v| v.animate_spin())
        .label("Loading")
}

/// What a list says when it has nothing, centered where the list would be.
pub fn empty(message: impl IntoReactive<SharedString>) -> View {
    view().flex_1().items_center().justify_center().p_6().child(text(message).text_center().text_color(theme().muted_foreground))
}
