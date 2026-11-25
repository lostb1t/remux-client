//! A row of chips to choose one of (Remux's filter chips on Home): white when chosen, glass when
//! not. NSH's `tabs` brings the arrow keys and what screen readers say.

use nsh_base::{tab, tabs, theme};
use nsh_core::prelude::*;

use super::tokens::{glass, motion};

/// Chips for `options`, the one `selected` says chosen; `on_select` with the one pressed. In a
/// sideways scroll view `row`, the chosen one is scrolled to as the chips are made: of a long row
/// (a show's 38 seasons) it may be far to the right.
pub fn chips<T: Clone + PartialEq + 'static>(
    options: Vec<(T, &'static str)>,
    selected: impl Fn() -> T + Copy + 'static,
    on_select: impl Fn(T) + Copy + 'static,
    row: Option<ElementRef>,
) -> impl IntoElement {
    tabs().flex_row().gap_2().children(options.into_iter().map(move |(value, label)| {
        let chosen = {
            let value = value.clone();
            move || selected() == value
        };
        // Only the row scrolls, once, to where the chosen one was laid out: `reveal` would scroll
        // the page too.
        let scrolled = stored(false);
        let first_chosen = chosen();
        tab()
            .accessibility_label(label.clone())
            .on_layout(move |bounds: &Bounds<Points>| {
                let (Some(row), true) = (row, first_chosen) else { return };
                if scrolled.with(|done| *done) {
                    return;
                }
                scrolled.with(|done| *done = true);
                let middle = bounds.origin.x.0 + bounds.size.width.0 / 2.0 - window().width.0 / 2.0;
                row.scroll_to_offset(px(middle.max(0.0)), false);
            })
            .selected(chosen.clone())
            .on_press(move |_: &()| on_select(value.clone()))
            .h(px(36.0))
            .px_4()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(glass())
            .border_1()
            .border_color(theme().border)
            .transition_colors()
            .duration(motion::INSTANT)
            .selected_style(|s| s.bg(theme().primary))
            .active(|s| s.scale(motion::PRESS_SCALE))
            .focus_visible(|s| s.outline(px(3.0)).outline_offset(px(2.0)).outline_color(theme().ring))
            .child(
                text(label)
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(move || if chosen() { theme().primary_foreground } else { theme().foreground }),
            )
    }))
}
