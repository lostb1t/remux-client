//! What stands where something still loads: a grey that slowly breathes (Remux's `.skeleton`).

use nsh_core::prelude::*;

use super::tokens::skeleton;

/// A block of `width` × `height` points that pulses while its content loads; still with reduced
/// motion.
pub fn skeleton_box(width: Length, height: Length) -> View {
    view()
        .w(width)
        .h(height)
        .rounded_lg()
        .bg(skeleton())
        .when(!reduce_motion(), |v| v.animate_pulse())
}
