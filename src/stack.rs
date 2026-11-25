//! The pages of a place on a phone: its first page, and the pages opened over it as layers, each
//! sliding in from the right. The page under the top one stays shown, so a swipe from the left
//! edge drags the top page away over it (Remux's swipe back), and Back slides it away the same
//! way. NSH's `nav_stack` shows the page below only for Android's own back gesture, which a phone
//! with buttons does not have; a TV keeps `nav_stack` (`shell.rs`), which gives the remote's
//! focus back to where a page was opened from.

use std::cell::RefCell;
use std::collections::HashMap;

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::nav::{self, Place};

/// How long a page takes to slide in or away (Android's own is 300 ms).
const SLIDE_MS: f32 = 280.0;
/// How far the page has to be dragged to go, of the screen's width (Remux: 30%).
const COMMIT: f32 = 0.3;
/// A flick this fast goes too, however short (points a second).
const FLICK: f32 = 800.0;

thread_local! {
    /// Where each layer is, by place and depth (1 is the first page over the place).
    static LAYERS: RefCell<HashMap<(Place, usize), AnimatedValue>> = RefCell::new(HashMap::new());
}

/// A page: its content on the page's background, filling it.
pub fn page(content: impl IntoElement) -> AnyElement {
    view().flex_1().bg(theme().background).child(content).into_element()
}

/// Slides the layer at `depth` of `place` away from `at` (where it is now), then takes it off.
fn slide_away(place: Place, depth: usize, at: f32, velocity: f32) {
    let Some(offset) = LAYERS.with(|layers| layers.borrow().get(&(place, depth)).copied()) else {
        nav::pop();
        return;
    };
    let width = window().width.0;
    // A flick goes at its own speed, at least as fast as the slide.
    let left = (width - at).max(0.0);
    let ms = if velocity > 0.0 { (left / velocity * 1000.0).clamp(80.0, SLIDE_MS) } else { SLIDE_MS * left / width.max(1.0) };
    let gone = offset.timing(width, ms.max(80.0), Easing::EaseOut);
    spawn(async move {
        gone.await;
        // Only when it is still the top page (Back pressed twice: one pop each).
        if nav::depth(place) == depth {
            nav::pop();
        }
    });
}

/// Back on a phone: the top page over the place shown slides away. Whether there was one.
pub fn back() -> bool {
    let place = nav::place();
    let depth = nav::depth(place);
    if depth == 0 {
        return false;
    }
    slide_away(place, depth, 0.0, 0.0);
    true
}

/// The layer of a page opened over `place`, at `depth`: it slides in as it is made, and a swipe
/// from the left edge takes it away.
pub fn layer(place: Place, depth: usize, content: AnyElement) -> impl IntoElement {
    let width = window().width.0;
    let offset = animated_value(width);
    LAYERS.with(|layers| layers.borrow_mut().insert((place, depth), offset));
    cleanup(move || {
        LAYERS.with(|layers| {
            let mut layers = layers.borrow_mut();
            if layers.get(&(place, depth)) == Some(&offset) {
                layers.remove(&(place, depth));
            }
        });
    });
    offset.timing(0.0, SLIDE_MS, Easing::EaseOut);
    view()
        .absolute()
        .inset_0()
        .bind_translate_x(&offset)
        .pan_activation(PanActivation::default().edge(PanEdge::Left))
        .pan_values(Some(&offset), None)
        .on_pan_end(move |event: &PanEvent| {
            let dragged = event.translation.x.0;
            if !event.cancelled && (dragged > width * COMMIT || (dragged > 0.0 && event.velocity_x > FLICK)) {
                slide_away(place, depth, event.value_x.unwrap_or(dragged), event.velocity_x);
            } else {
                offset.timing(0.0, 180.0, Easing::EaseOut);
            }
        })
        // Only the top page and the one under it are shown: the others are covered anyway.
        .style_when(move || nav::depth(place) > depth + 1, |s| s.invisible())
        .child(content)
}

/// Whether the first page of `place` is shown (under at most one layer).
pub fn root_shown(place: Place) -> impl Fn() -> bool + Copy {
    move || nav::depth(place) <= 1
}
