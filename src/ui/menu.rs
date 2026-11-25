//! Rows of choices in a group, as a sheet or a settings page has them (Remux's `MenuList`,
//! `MenuChoice`, `MenuRow`, `MenuHeading`, `MenuNote`): a rounded group, hairlines between the rows.

use nsh_base::{button, switch, theme};
use nsh_core::prelude::*;
use nsh_icon::icon;
use nsh_icon_phosphor::Phosphor;

use super::tokens::{group, motion};

/// The rows' text size (Remux's 17 px, iOS's body).
const ROW_TEXT: f32 = 17.0;

/// A group of `rows`, with a hairline between each two.
pub fn menu_group(rows: Vec<AnyElement>) -> View {
    let last = rows.len().saturating_sub(1);
    view().rounded(px(26.0)).overflow_hidden().bg(group()).children(rows.into_iter().enumerate().map(move |(at, row)| {
        view()
            .child(row)
            .when(at < last, |v| v.child(view().absolute().left(px(16.0)).right(px(16.0)).bottom_0().h(px(1.0)).bg(theme().border)))
    }))
}

/// A row that does something: an icon (optional) and a label; `chevron` when it opens more.
pub fn menu_row(glyph: Option<nsh_icon::Icon>, label: impl IntoReactive<SharedString>, chevron: bool, on_press: impl Fn() + 'static) -> AnyElement {
    menu_row_at(None, glyph, label, chevron, on_press)
}

/// A [`menu_row`] that `target` names, to move the focus to.
pub fn menu_row_at(
    target: Option<ElementRef>,
    glyph: Option<nsh_icon::Icon>,
    label: impl IntoReactive<SharedString>,
    chevron: bool,
    on_press: impl Fn() + 'static,
) -> AnyElement {
    // The row says what it is to a screen reader (and to the e2e tests), not only its text.
    let label = label.into_reactive();
    let spoken = label.clone();
    // A row that opens a page: Back comes back to it.
    let me = target.unwrap_or_else(element_ref);
    let on_press = move || {
        crate::tv_focus::pressed(me);
        on_press();
    };
    let row = button().accessibility_label(move || Some(spoken.get())).element_ref(&me);
    row
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .min_h(px(48.0))
        .text_color(theme().foreground)
        .transition_colors()
        .duration(motion::INSTANT)
        .active(|s| s.bg(theme().accent))
        .focus_visible(focused)
        .on_press(on_press)
        .child(glyph.map(|glyph| icon(glyph).size(px(19.0))))
        .child(text(label).flex_1().min_w(px(0.0)).number_of_lines(1).text_size(px(ROW_TEXT)))
        .child(chevron.then(|| icon(Phosphor::CaretRight).size(px(18.0)).text_color(theme().muted_foreground)))
        .into_element()
}

/// A row with a switch: the row is the switch's label, a press anywhere on it turns it.
pub fn switch_row(label: impl Into<SharedString>, on: impl Fn() -> bool + 'static, on_change: impl Fn(bool) + 'static) -> AnyElement {
    switch()
        .checked(on)
        .on_change(move |next: &bool| on_change(*next))
        // The switch draws its track before its label: reversed, the label is at the start and
        // the switch at the end of the row, as in Remux.
        .flex_row_reverse()
        .items_center()
        .justify_between()
        .gap_3()
        .px_4()
        .py_3()
        .min_h(px(48.0))
        .text_color(theme().foreground)
        .active(|s| s.bg(theme().accent))
        .focus_visible(focused)
        .track_style(|s| {
            s.w(px(44.0)).h(px(24.0)).rounded_full().p(px(4.0)).bg(theme().foreground.opacity(0.2)).transition_colors().duration(motion::FAST).checked(|s| s.bg(super::tokens::positive()))
        })
        .thumb_style(|s| s.size(px(16.0)).rounded_full().bg(theme().foreground).transition_transform().duration(motion::FAST).checked(|s| s.translate_x(px(20.0))))
        .child(text(label.into()).flex_1().text_size(px(ROW_TEXT)))
        .into_element()
}

/// A row with a remote's focus: white with dark text on a TV, as in Remux (a darker grey is
/// hard to see from a couch); a lighter grey elsewhere.
fn focused(s: nsh_core::element::StyleRefinement) -> nsh_core::element::StyleRefinement {
    if super::tokens::is_tv() {
        s.bg(theme().primary).text_color(theme().primary_foreground)
    } else {
        s.bg(theme().accent)
    }
}

/// The title over a group.
pub fn menu_heading(label: impl Into<SharedString>) -> Text {
    text(label.into()).px_4().text_size(px(15.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme().muted_foreground)
}

/// What explains a group, under it.
pub fn menu_note(note: impl Into<SharedString>) -> Text {
    text(note.into()).px_4().text_size(px(14.0)).text_color(theme().muted_foreground)
}

/// A row of a choice: its name, what it is under it (a version's video, a device's model), and
/// a check when it is the one chosen.
pub fn menu_choice_checked(title: impl Into<SharedString>, detail: Option<String>, checked: bool, on_press: impl Fn() + 'static) -> AnyElement {
    let title: SharedString = title.into();
    button()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .min_h(px(56.0))
        .text_color(theme().foreground)
        .accessibility_label(title.clone())
        .transition_colors()
        .duration(motion::INSTANT)
        .active(|s| s.bg(theme().accent))
        .focus_visible(focused)
        .on_press(on_press)
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .child(text(title).number_of_lines(1).text_size(px(ROW_TEXT)).font_weight(FontWeight::SEMIBOLD))
                .child(detail.map(|detail| text(detail).number_of_lines(1).text_size(px(14.0)).opacity(0.6))),
        )
        .child(checked.then(|| icon(Phosphor::Check).size(px(18.0))))
        .into_element()
}

/// A row of a choice without a check.
pub fn menu_choice(title: impl Into<SharedString>, detail: Option<String>, on_press: impl Fn() + 'static) -> AnyElement {
    menu_choice_checked(title, detail, false, on_press)
}
