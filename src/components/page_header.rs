//! The top of a page that is no hero: a way back (a TV has its remote's Back) and its title.

use nsh_base::theme;
use nsh_core::prelude::*;
use nsh_icon_phosphor::Phosphor;

use crate::ui::{self, tokens::is_tv};

/// `title`, with Back before it when `back` (a page opened over a place).
pub fn page_header(title: impl Into<SharedString>, back: bool) -> View {
    // A TV's first page of a place: room for the menu in the top left corner, which says the
    // place already (Remux's `--top-bar: 5rem`); no title of its own.
    if is_tv() && !back {
        return view().h(px(88.0));
    }
    view()
        // A page over a place on a TV: no menu over it, a little room at the top.
        .pt_safe_area_plus(px(if is_tv() { 48.0 } else { 16.0 }))
        .pb_4()
        .flex_row()
        .items_center()
        .gap_3()
        .child((back && !is_tv()).then(|| ui::icon_button(ui::Size::Regular, Phosphor::CaretLeft, "Back", || { crate::stack::back(); }).test_id("page-back")))
        .child(text(title.into()).flex_1().text_2xl().font_weight(FontWeight::BOLD).number_of_lines(1).text_color(theme().foreground))
}
