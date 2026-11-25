//! A sheet from the bottom of the screen (Remux's `Sheet`): a dark panel with rounded top corners,
//! its title in the middle of its top and a round close button at the left, over a scrim. NSH's
//! `sheet` brings the slide, the swipe down, Escape and Android's back.

use nsh_base::{SheetEvent, button, sheet as base_sheet, theme};
use nsh_core::prelude::*;
use nsh_icon::labeled_icon;
use nsh_icon_phosphor::Phosphor;

use super::tokens::{motion, scrim, sheet as sheet_color};

/// A sheet titled `title`, open while `open` is; `content` is made each time it opens.
pub fn sheet<E: IntoElement>(open: Signal<bool>, title: impl Into<SharedString>, content: impl Fn() -> E + 'static) -> impl IntoElement {
    sheet_focusing(open, title, None, content)
}

/// A [`sheet`] whose remote's focus starts on `first` (a menu's first choice), not on its close
/// button.
pub fn sheet_focusing<E: IntoElement>(
    open: Signal<bool>,
    title: impl Into<SharedString>,
    first: Option<ElementRef>,
    content: impl Fn() -> E + 'static,
) -> impl IntoElement {
    let title: SharedString = title.into();
    // A remote's focus goes into the sheet as it opens (to its close button, at the top; down
    // goes on to the rows): what had it is under the sheet, or gone.
    let close = element_ref();
    if super::tokens::is_tv() {
        effect(move || {
            if open.get() {
                set_timeout(std::time::Duration::from_millis(700), move || first.unwrap_or(close).focus());
            }
        });
    }
    base_sheet()
        .open(move || Some(open.get()))
        .on_open_change(move |event: &SheetEvent| {
            if let SheetEvent::OpenChange(next, _) = event {
                open.set(*next);
            }
        })
        .close_on_swipe(true)
        .accessibility_label(title.clone())
        .backdrop_style(|s| s.bg(scrim()))
        .popup_style(|s| s.bg(sheet_color()).rounded_t(px(26.0)).border_1().border_color(theme().border).max_h(relative_screen(0.85)))
        .child(
            view()
                .max_h(relative_screen(0.85))
                .pb_safe_area_plus(px(32.0))
                .child(
                    view()
                        .h(px(64.0))
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .px_4()
                        .child(
                            button()
                                .element_ref(&close)
                                .absolute()
                                .left(px(16.0))
                                .size(px(40.0))
                                .rounded_full()
                                .items_center()
                                .justify_center()
                                .border_1()
                                .border_color(theme().border)
                                .bg(scrim())
                                .text_color(theme().foreground)
                                .accessibility_label("Close")
                                .transition_transform()
                                .duration(motion::FAST)
                                .active(|s| s.scale(motion::ICON_PRESS_SCALE))
                                .focus_visible(|s| s.bg(theme().primary).text_color(theme().primary_foreground))
                                .on_press(move || open.set(false))
                                .child(labeled_icon(Phosphor::X, "Close").size(px(22.0))),
                        )
                        .child(text(title).text_size(px(17.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme().foreground)),
                )
                // What does not fit scrolls, and follows a remote's focus down.
                .child(scroll().flex_shrink(1.0).child(view().px_4().gap_4().child(dynamic(move || content().into_element())))),
        )
}

/// A share of the window's height, in points.
fn relative_screen(share: f32) -> Length {
    px(window().height.0 * share).into()
}
