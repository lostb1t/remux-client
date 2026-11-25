//! A title's page (Remux's `MediaDetailView`): its hero, what it is about, its seasons and
//! episodes for a show, and its cast. It opens with what a list loaded of the title, and the page's
//! own request (with the cast) replaces that when it is there.

mod seasons;

use nsh_base::{button, theme};
use nsh_core::prelude::*;
use nsh_icon::labeled_icon;
use nsh_icon_phosphor::Phosphor;
use uuid::Uuid;

use crate::components::{cast_row, hero_item, media_info, HeroKind};
use crate::hooks::server;
use crate::media::{self, Media, MediaType};
use crate::nav;
use crate::tv_focus;
use crate::ui::{self, tokens::{is_tv, is_wide, motion, scrim}};

/// The height of the page's hero: a phone shows the story just below it (Remux: 60% of the screen,
/// `lg:min-h-140` on a wide one).
fn hero_height() -> f32 {
    if is_wide() { 560.0 } else { window().height.0 * 0.6 }
}

/// The page of the title `id`.
pub fn detail(id: Uuid) -> AnyElement {
    let focus = tv_focus::Scope::provide();
    let page = element_ref();
    focus.set_page(page);
    // Loaded again when the title was watched, marked or favorited.
    let details = resource(
        move || Some(crate::user_data::last().ids.contains(&id)),
        move |_, _| async move { server().media_details(id).await.map_err(|error| error.to_string()) },
    );
    let item = move || details.get().flatten().or_else(|| media::recent(id));
    crate::stack::page(
        view()
            .flex_1()
            .bg(theme().background)
            .child(
                scroll().flex_1().scrollbar(false).element_ref(&page).child(dynamic(move || match item() {
                    Some(item) => untracked(move || content(item, details.get().flatten().is_some()).into_element()),
                    None => match details.error() {
                        Some(error) => ui::empty(format!("Could not load it: {error}")).pt(px(120.0)).into_element(),
                        None if details.loading() => view().h(px(400.0)).items_center().justify_center().child(ui::spinner()).into_element(),
                        None => ui::empty("Not found").pt(px(120.0)).into_element(),
                    },
                })),
            )
            // The way back on a phone; a TV has its remote's Back.
            .child((!is_tv()).then(back_button)),
    )
}

/// A round glass Back at the top left, over the hero.
fn back_button() -> impl IntoElement {
    view().absolute().top_0().left_0().pt_safe_area_plus(px(12.0)).pl(px(16.0)).child(
        button()
            .size(px(40.0))
            .rounded_full()
            .items_center()
            .justify_center()
            .border_1()
            .border_color(theme().border)
            .bg(scrim())
            .text_color(theme().foreground)
            .accessibility_label("Back")
            .transition_transform()
            .duration(motion::FAST)
            .active(|s| s.scale(motion::ICON_PRESS_SCALE))
            .on_press(|| { crate::stack::back(); })
            .child(labeled_icon(Phosphor::CaretLeft, "Back").size(px(22.0))),
    )
}

/// The page of `item`: `full` once its own request is in (which brings the cast).
fn content(item: Media, full: bool) -> impl IntoElement {
    let open = nav::open_media;
    let series = item.is_series();
    view()
        .pb(crate::ui::tokens::bottom_room(140.0))
        .child(view().w_full().h(px(hero_height())).child(hero_item(item.clone(), HeroKind::Page, true, open)))
        .child(media_info(&item))
        .child(series.then(|| seasons::seasons(item.clone(), 0)))
        .child(full.then(|| cast_row(&item, 1)).flatten())
        .child((item.media_type == MediaType::Episode).then(|| view()))
}
