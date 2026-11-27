//! The app's frame: a stack of pages for each place, and the navigation over them. A phone has a
//! glass bar at the bottom (Remux's `nav`); a TV a glass pill in the top left corner, where up
//! from the hero goes.

use nsh_base::{avatar, button, nav_page, nav_stack, theme};
use nsh_core::prelude::*;
use nsh_icon::icon;
use nsh_icon_phosphor::Phosphor;

use crate::nav::{self, Place, Route};
use crate::tv_focus;
use crate::pages::{detail::detail, discover, home::home, library, live, music};
use crate::ui::{self, tokens::{avatar as avatar_color, is_tv, motion}};

/// A place's icon in the tab bar.
fn glyph(place: Place) -> nsh_icon::Icon {
    match place {
        Place::Home => Phosphor::House,
        Place::Media => Phosphor::FilmStrip,
        Place::Discover => Phosphor::Compass,
        Place::LiveTv => Phosphor::Television,
    }
}

/// The first page of a place, under an owner of its own: what a page provides (its remote's
/// `tv_focus::Scope`) is its own, not the shell's, where the next place's would take its place.
fn place_page(place: Place) -> AnyElement {
    dynamic(move || untracked(move || place_page_now(place).into_element())).into_element()
}

fn place_page_now(place: Place) -> AnyElement {
    match place {
        Place::Home => home(),
        Place::Media => library::media(),
        Place::Discover => discover::discover(),
        Place::LiveTv => live::live_tv(),
    }
}

/// The page of a route opened over a place.
fn route_page(route: &Route) -> AnyElement {
    match route {
        Route::MediaDetail { id, .. } => detail(*id),
        Route::Library { id, name } => library::library(*id, name.clone()),
        Route::Collection { id, name } => library::collection(*id, name.clone()),
        Route::Favorites => library::favorites(),
        Route::Playlists => library::playlists(),
        Route::Person { id, name } => library::person(*id, name.clone()),
        Route::Album { id, name } => music::album(*id, name.clone()),
        Route::LiveChannels => live::channels(),
        Route::LiveGuide => live::guide(),
        Route::Artist { id, name } => music::artist(*id, name.clone()),
        Route::Playlist { id, name } => music::playlist(*id, name.clone()),
    }
}

/// The pages with their navigation. Every place keeps its stack while another one shows, with
/// where it was scrolled to, as a tab bar does.
pub fn shell() -> impl IntoElement {
    // The device profile and the speed of the line, for the server (`profile.rs`).
    crate::profile::start();
    // The web: an address for each page, which the browser's back, forward and reload follow.
    #[cfg(target_arch = "wasm32")]
    crate::address::start();
    // Chromecasts on the network, to cast to from a phone.
    if !is_tv() {
        crate::cast::start();
    }
    // Back on the first page of another place goes Home, as in Remux; on Home's it leaves the app.
    on_back_press(|| {
        // A phone's pages over a place slide away (a TV's `nav_stack` takes Back itself).
        if !is_tv() && crate::stack::back() {
            return true;
        }
        let home = nav::place() == Place::Home;
        if home || nav::can_go_back() {
            return false;
        }
        nav::go(Place::Home);
        true
    });
    view()
        .flex_1()
        .children(Place::ALL.map(|place| {
            view()
                .absolute()
                .inset_0()
                // Invisible, not hidden: a hidden place has no size, and its rows' native lists
                // lose their cells for good (seen on Android with NSH 0.7.0).
                .style_when(move || nav::place() != place, |s| s.invisible().pointer_events(PointerEvents::None))
                // Not read out either: a screen reader (and the e2e flows) would find what is on
                // the places that do not show.
                .accessibility_elements_hidden(move || nav::place() != place)
                .child(if is_tv() {
                    // A TV: NSH's stack, which gives the remote's focus back to where a page was
                    // opened from.
                    nav_stack()
                        .flex_1()
                        .child(nav_page().header_visible(false).child(place_page(place)))
                        .child(each(
                            move || nav::routes(place),
                            |route: &Route| route.key(),
                            move |route, index| {
                                // The user went back natively: this route goes, and those above it.
                                nav_page()
                                    .header_visible(false)
                                    .child(route_page(&route.get_untracked()))
                                    .on_dismiss(move || nav::truncate(place, index.get_untracked()))
                            },
                        ))
                        .into_element()
                } else {
                    // A phone: the app's own stack, with the page below shown under a swipe.
                    view()
                        .flex_1()
                        .child(
                            view()
                                .absolute()
                                .inset_0()
                                .style_when(move || !crate::stack::root_shown(place)(), |s| s.invisible())
                                .accessibility_elements_hidden(move || crate::nav::depth(place) > 0)
                                .child(place_page(place)),
                        )
                        .child(each(
                            move || nav::routes(place),
                            |route: &Route| route.key(),
                            move |route, index| crate::stack::layer(place, index.get_untracked() + 1, route_page(&route.get_untracked())),
                        ))
                        .into_element()
                })
        }))
        // The places: the menu at the top left (a TV, a wide window), else the tab bar at the bottom.
        .child(dynamic(|| if crate::ui::tokens::has_menu() { untracked(|| tv_menu().into_element()) } else { untracked(|| tab_bar().into_element()) }))
        .child(crate::components::music_host())
        .child(crate::components::item_menu_host())
        .child(crate::components::play_choice_host())
        .child(crate::components::settings_host())
        .child(crate::pages::player::player_host())
}

/// The user's round, with their initial.
fn user_avatar(size: f32) -> impl IntoElement {
    avatar()
        .name("S")
        .size(px(size))
        .fallback_style(move |s| {
            s.rounded_full()
                .items_center()
                .justify_center()
                .bg(avatar_color())
                .text_color(theme().foreground)
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
        })
}

/// The glass bar at the bottom of a phone.
fn tab_bar() -> impl IntoElement {
    let tab = |place: Place| {
        let current = move || nav::place() == place;
        button()
            .flex_1()
            .min_w(px(0.0))
            .h(px(52.0))
            .items_center()
            .justify_center()
            .gap(px(2.0))
            .rounded_full()
            .selected(current)
            // For the e2e flows (Maestro's `id:`): a name of its own, apart from the system's own
            // Home and Back buttons, which Android shows with the same names.
            .test_id(&format!("tab-{}", place.label().to_lowercase().replace(' ', "-")))
            .accessibility_label(place.label())
            .text_color(move || secondary_text_or(current()))
            .transition_colors()
            .duration(motion::INSTANT)
            .selected_style(|s| s.bg(theme().accent).text_color(theme().foreground))
            .active(|s| s.bg(theme().accent).scale(motion::PRESS_SCALE))
            .on_press(move || nav::go(place))
            .child(icon(glyph(place)).size(px(22.0)))
            .child(text(place.label()).text_size(px(11.0)).font_weight(FontWeight::MEDIUM).number_of_lines(1))
    };
    view()
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .items_center()
        .px_3()
        .pb_safe_area_plus(px(8.0))
        .child(
            ui::glass_surface()
                .w_full()
                .max_w(px(448.0))
                .flex_row()
                .items_center()
                .gap_1()
                .p(px(6.0))
                .rounded_full()
                .children(Place::ALL.map(tab))
                .child(
                    button()
                        .flex_1()
                        .h(px(52.0))
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .test_id("tab-settings")
                        .accessibility_label("Settings")
                        .active(|s| s.bg(theme().accent))
                        .on_press(crate::components::open_settings)
                        .child(user_avatar(24.0)),
                ),
        )
}

/// The text color of a tab: white when it is the current place, else a little dimmer.
fn secondary_text_or(current: bool) -> AdaptiveColor {
    if current { theme().foreground } else { theme().foreground.opacity(0.8) }
}

/// A TV's menu: a glass pill in the top left corner with the user and where they are.
fn tv_menu() -> impl IntoElement {
    let target = element_ref();
    tv_focus::set_menu(target);
    tv_focus::give_back();
    let open = signal(false);
    // Whether Back opened it (at the top of a page): Back again leaves (Home) or goes Home. Opened
    // by the remote coming to it, Back closes it, back into the page.
    let by_back = stored(false);
    // The rows of the panel, in order: the places, then Settings. The remote goes along them
    // itself, so it never falls back onto the pill (which would open the panel again).
    let rows: [ElementRef; 5] = std::array::from_fn(|_| element_ref());
    let current_row = move || Place::ALL.iter().position(|place| *place == nav::place()).unwrap_or(0);
    let at = stored(0usize);
    let open_panel = move |back: bool| {
        if open.get_untracked() {
            return;
        }
        by_back.with(|by| *by = back);
        open.set(true);
        let row = current_row();
        at.with(|at| *at = row);
        set_timeout(std::time::Duration::from_millis(50), move || rows[row].focus());
    };
    let close_to_page = move || {
        open.set(false);
        let place = nav::place();
        tv_focus::to_first_later(place);
    };
    // Back on a TV, on the first page of a place (Remux: the newest thing closes first, and only
    // the first page leaves): away from the top it goes up; at the top it opens the menu; with
    // the menu open it leaves the app on Home, else it goes Home. A page over the place is
    // `nav_stack`'s to close.
    on_back_press(move || {
        let place = nav::place();
        if open.get_untracked() {
            if !by_back.with(|by| *by) {
                close_to_page();
                return true;
            }
            if place == Place::Home {
                return false;
            }
            open.set(false);
            nav::go(Place::Home);
            tv_focus::to_first_later(Place::Home);
            return true;
        }
        if nav::can_go_back() {
            return false;
        }
        if !tv_focus::at_top(place) {
            tv_focus::to_top(place);
            return true;
        }
        open_panel(true);
        true
    });
    // A place: the panel closes, and the remote starts on the place's first element (once the
    // place shows: a view that is not shown takes no focus).
    let go = move |place: Place| {
        open.set(false);
        nav::go(place);
        tv_focus::to_first_later(place);
    };
    view()
        .absolute()
        .top(px(20.0))
        .left(px(24.0))
        .gap_3()
        // Only on the first page of a place, at its top (and while the panel is open): it would
        // stand over the page's rows, and a page over the place has Back.
        .style_when(move || !open.get() && (nav::can_go_back() || !tv_focus::at_top(nav::place())), |s| s.invisible().pointer_events(PointerEvents::None))
        .child(
            // The pill: the remote coming to it opens the panel (a view with a button's role, not
            // `button`: it takes the focus event, which `button` has not).
            view()
                .self_start()
                .element_ref(&target)
                .focusable(true)
                .role(AccessibilityRole::Button)
                .label("Menu")
                .rounded_full()
                .focus_visible(|s| s.outline(px(4.0)).outline_offset(px(3.0)).outline_color(theme().ring))
                .on_focus(move || open_panel(false))
                .on_press(move || {
                    if open.get_untracked() {
                        close_to_page();
                    } else {
                        open_panel(false);
                    }
                })
                .child(
                    ui::glass_surface()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .py_2()
                        .pl_3()
                        .pr_6()
                        .rounded_full()
                        .child(user_avatar(28.0))
                        .child(text(move || nav::place().label()).text_lg().font_weight(FontWeight::SEMIBOLD).text_color(theme().foreground)),
                ),
        )
        // The panel: the places, and the settings (Remux's TV menu).
        .child(when(
            move || open.get(),
            move || {
                ui::glass_surface()
                    .w(px(384.0))
                    .rounded(px(32.0))
                    .p_3()
                    .gap_3()
                    // The remote stays in the panel: up and down along its rows (not onto the pill,
                    // nor out to the page); left and right stay (Remux).
                    .capture_keys(tv_focus::capture(&["up", "down", "left", "right"]))
                    .on_key_down(move |event: &nsh_core::keyboard::KeyEvent| {
                        let now = at.with(|at| *at);
                        let next = match event.key.as_ref() {
                            "ArrowUp" => now.saturating_sub(1),
                            "ArrowDown" => (now + 1).min(rows.len() - 1),
                            _ => return,
                        };
                        at.with(|at| *at = next);
                        rows[next].focus();
                    })
                    .child(ui::menu_group(
                        Place::ALL
                            .into_iter()
                            .enumerate()
                            .map(|(index, place)| ui::menu_row_at(Some(rows[index]), Some(glyph(place)), place.label(), false, move || go(place)))
                            .collect(),
                    ))
                    .child(ui::menu_group(vec![ui::menu_row_at(Some(rows[4]), Some(Phosphor::GearSix), "Settings", true, move || {
                        open.set(false);
                        crate::components::open_settings();
                    })]))
            },
        ))
}
