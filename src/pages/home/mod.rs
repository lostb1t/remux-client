//! The home screen: the hero and the rows under it, in one scroll view that owns the page.

mod filters;
mod hero;
mod rows;

use nsh_core::prelude::*;

use crate::nav;

/// The home page: no header, the hero goes up to the top of the screen.
pub fn home() -> AnyElement {
    let on_open = nav::open_media;
    let focus = crate::tv_focus::Scope::provide();
    let page = element_ref();
    focus.set_page(page);
    crate::stack::page(scroll()
        .element_ref(&page)
        .on_scroll(crate::tv_focus::root_scroll(nav::Place::Home, page))
        .flex_1()
        .scrollbar(false)
        .child(
            view()
                .gap_6()
                // The hero is for the films and shows; music has rows of its own.
                .child(dynamic(move || {
                    let music = filters::music();
                    let types = filters::home_types();
                    untracked(move || if music { view().into_element() } else { hero::hero(types, on_open).into_element() })
                }))
                .child(rows::rows(on_open)),
        )
        // On a phone, the filters over the top of the page (a TV has them in its menu).
        .child((!crate::ui::tokens::is_tv()).then(filters::home_menu)))
}
