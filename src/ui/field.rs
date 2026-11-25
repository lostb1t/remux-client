//! A text field (Remux's `MenuInput`): a pill on the group color, the text at the size of a row.

use nsh_base::{input, theme};
use nsh_core::prelude::*;
use nsh_core::{AutoCapitalize, AutoFill, KeyboardType};

use super::tokens::group;

/// What a field holds: it sets the keyboard, the masking and what the system may fill in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    /// A server's address.
    Url,
    Username,
    Password,
}

/// A field with `placeholder`, controlled by `value`; `on_submit` on Return.
pub fn field(value: Signal<String>, placeholder: impl Into<String>, on_submit: impl Fn(&str) + 'static) -> impl IntoElement {
    field_of(FieldKind::Text, value, placeholder, on_submit)
}

/// A field for `kind` of text.
pub fn field_of(kind: FieldKind, value: Signal<String>, placeholder: impl Into<String>, on_submit: impl Fn(&str) + 'static) -> impl IntoElement {
    let me = element_ref();
    let mut field = input()
        .element_ref(&me)
        .value(move || value.get())
        .on_change(move |text: &str| value.set(text.to_owned()))
        .on_submit(move |text: &str| {
            // A TV's remote goes on from the field once it is sent (the keyboard goes with its
            // focus): what was asked for is below it.
            if super::tokens::is_tv() {
                me.blur();
            }
            on_submit(text)
        })
        .placeholder(placeholder.into())
        .px_4()
        .py(px(14.0))
        .rounded_full()
        .bg(group())
        .text_size(px(17.0))
        .text_color(theme().foreground)
        // The app is dark only: the dark value.
        .placeholder_color(theme().foreground.opacity(0.3).dark);
    field = match kind {
        FieldKind::Text => field,
        FieldKind::Url => field.keyboard_type(KeyboardType::Url).auto_capitalize(AutoCapitalize::None).auto_fill(AutoFill::Url),
        FieldKind::Username => field.auto_capitalize(AutoCapitalize::None).auto_fill(AutoFill::Username),
        FieldKind::Password => field.masked(true).auto_fill(AutoFill::Password),
    };
    field
}
