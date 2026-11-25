//! Frosted glass for bars that float over the page (Remux's `.bar-surface`): a dark, translucent
//! fill, blurred behind except on a TV, a thin light edge and a soft shadow.

use nsh_base::theme;
use nsh_core::prelude::*;

use super::tokens::{glass, glass_highlight, is_tv};

/// A glass surface; the caller gives it its shape and content.
pub fn glass_surface() -> View {
    view()
        .bg(glass())
        .border_1()
        .border_color(theme().border)
        .inset_ring(px(1.0))
        .inset_ring_color(glass_highlight())
        .shadow_lg()
        .when(!is_tv(), |v| v.backdrop_blur(px(24.0)))
}
