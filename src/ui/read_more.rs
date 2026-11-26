//! A long text cut short, with a button to read the rest (Remux's `ReadMore`): a person's or an
//! artist's story.

use nsh_base::{button, theme};
use nsh_core::prelude::*;

/// Longer than this is cut to six lines.
const LONG: usize = 280;

pub fn read_more(story: String) -> impl IntoElement {
    let long = story.chars().count() > LONG;
    let open = signal(false);
    view()
        .gap_2()
        .child(
            text(story)
                .text_base()
                .line_height(px(26.0))
                .text_color(theme().foreground)
                .number_of_lines(move || if long && !open.get() { 6u32 } else { 0u32 }),
        )
        .child(long.then(|| {
            button()
                .self_start()
                .rounded_md()
                .px_1()
                .accessibility_label(move || Some(SharedString::from(if open.get() { "Show less" } else { "Read more" })))
                .focus_visible(|s| s.bg(theme().primary).text_color(theme().primary_foreground))
                .text_color(theme().muted_foreground)
                .on_press(move || open.update(|open| *open = !*open))
                .child(text(move || if open.get() { "Show less" } else { "Read more" }).text_sm().font_weight(FontWeight::MEDIUM))
        }))
}
