//! Every component of the `ui` layer in each variant, size and state, on one page: where a new
//! component is looked at before features use it (Design Guides, "The app's ui layer").

use nsh_base::{theme};
use nsh_core::prelude::*;
use nsh_icon_phosphor::Phosphor;

use crate::nav;
use crate::ui::{self, Size, Variant};

/// The gallery.
pub fn gallery() -> AnyElement {
    let sheet_open = signal(false);
    let chip = signal("all");
    let draft = signal(String::new());
    let switch_on = signal(true);
    let section = |title: &'static str, content: View| {
        view().gap_3().child(text(title).text_lg().font_weight(FontWeight::SEMIBOLD).text_color(theme().foreground)).child(content)
    };
    crate::stack::page(
        scroll().flex_1().bg(theme().background).child(
            view()
                .pt_safe_area_plus(px(16.0))
                .pb(crate::ui::tokens::bottom_room(140.0))
                .px_6()
                .gap_8()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .child(ui::icon_button(Size::Regular, Phosphor::CaretLeft, "Back", nav::pop))
                        .child(text("Components").text_2xl().font_weight(FontWeight::BOLD).text_color(theme().foreground)),
                )
                .child(section(
                    "Buttons",
                    view()
                        .gap_3()
                        .items_start()
                        .child(ui::text_button(Variant::Primary, Size::Regular, Phosphor::PlayFill, "Play", || {}))
                        .child(ui::text_button(Variant::Primary, Size::Large, Phosphor::PlayFill, "Play, large", || {}))
                        .child(ui::text_button(Variant::Secondary, Size::Regular, Phosphor::Shuffle, "Secondary", || {}))
                        .child(ui::text_button(Variant::Primary, Size::Regular, Phosphor::PlayFill, "Disabled", || {}).disabled(true))
                        .child(
                            view()
                                .flex_row()
                                .gap(px(10.0))
                                .child(ui::icon_button(Size::Regular, Phosphor::Heart, "Favorite", || {}))
                                .child(ui::icon_button(Size::Regular, Phosphor::CheckCircle, "Watched", || {}))
                                .child(ui::icon_button(Size::Large, Phosphor::Info, "More info", || {})),
                        ),
                ))
                .child(section(
                    "Chips",
                    view().child(ui::chips(vec![("all", "All"), ("movies", "Movies"), ("shows", "Shows"), ("music", "Music")], move || chip.get(), move |value| chip.set(value))),
                ))
                .child(section("Field", view().child(ui::field(draft, "Search", |_| {}))))
                .child(section(
                    "Menu",
                    view()
                        .gap_2()
                        .child(ui::menu_heading("Playback"))
                        .child(ui::menu_group(vec![
                            ui::menu_row(Some(Phosphor::Heart), "Add to favorites", false, || {}),
                            ui::menu_row(Some(Phosphor::ListPlus), "Add to playlist", true, || {}),
                            ui::switch_row("Hide watched", move || switch_on.get(), move |on| switch_on.set(on)),
                        ]))
                        .child(ui::menu_note("What a note under a group says.")),
                ))
                .child(section(
                    "Sheet",
                    view().items_start().child(ui::text_button(Variant::Secondary, Size::Regular, Phosphor::DotsThree, "Open a sheet", move || sheet_open.set(true))),
                ))
                .child(section(
                    "Loading and empty",
                    view()
                        .gap_4()
                        .child(view().flex_row().gap_4().items_center().child(ui::spinner()).child(ui::skeleton_box(px(120.0).into(), px(64.0).into())))
                        .child(ui::empty("Nothing here yet.").h(px(80.0))),
                ))
                .child(ui::sheet(sheet_open, "More", move || {
                    ui::menu_group(vec![
                        ui::menu_row(Some(Phosphor::Heart), "Add to favorites", false, move || sheet_open.set(false)),
                        ui::menu_row(Some(Phosphor::CheckCircle), "Mark as watched", false, move || sheet_open.set(false)),
                        ui::menu_row(Some(Phosphor::ListPlus), "Add to playlist", true, move || sheet_open.set(false)),
                    ])
                })),
        ),
    )
}
