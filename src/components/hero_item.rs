//! A hero item: the big picture of a title with its logo, what it is and its buttons (Remux's
//! `HeroItem`). Home goes through several; a title's page has one at its top.

use nsh_base::theme;
use nsh_core::element::ImageSrc;
use nsh_core::geometry::relative;
use nsh_core::keyboard::KeyEvent;
use nsh_core::prelude::*;
use nsh_icon::icon;
use nsh_icon_phosphor::Phosphor;

use crate::hooks::server;
use crate::media::{ImageType, Media, MediaType, RatingSource};
use crate::tv_focus;
use crate::ui::{self, tokens::{is_tv, is_wide, rating_star, secondary_text}};

/// The button next to Play that was pressed last, of which title: the page reloads its title
/// when it is favorited, watched or put on the watchlist, which makes its hero again, and the
/// remote stays on that button instead of going back to Play.
thread_local! {
    static PRESSED: std::cell::Cell<Option<(uuid::Uuid, Side)>> = const { std::cell::Cell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Favorite,
    Watched,
    Watchlist,
}

/// Where a hero item is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HeroKind {
    /// One of Home's: its title opens its page (the info button on a wide screen).
    Home,
    /// The top of a title's own page.
    Page,
}

/// A hero item: its picture, fading into the page, and over it its logo or title, what it is, and
/// what can be done with it. `autofocus`: on a TV the remote starts on its play button (the first
/// of Home's, a title's page).
pub fn hero_item(media: Media, kind: HeroKind, autofocus: bool, on_open: impl Fn(Media) + Copy + 'static) -> impl IntoElement {
    let focus = tv_focus::Scope::current();
    let server = server();
    let backdrop = server
        .image_url(&media, ImageType::Backdrop)
        .or_else(|| server.image_url_at(&media, ImageType::Poster, 1280));
    let logo = server.image_url(&media, ImageType::Logo);
    let favorite = signal(media.is_favorite());
    let watched = signal(media.is_watched());
    let watchlist = signal(media.is_watchlist());
    let play_ref = element_ref();
    let (favorite_ref, watched_ref, watchlist_ref) = (element_ref(), element_ref(), element_ref());
    if is_tv() {
        focus.set_play(play_ref);
    }
    if autofocus && is_tv() {
        // The button pressed before this title was loaded again, else Play.
        let id = media.id;
        let again = PRESSED.with(|pressed| pressed.take()).filter(|(pressed, _)| *pressed == id).map(|(_, side)| side);
        let target = match again {
            Some(Side::Favorite) => favorite_ref,
            Some(Side::Watched) => watched_ref,
            Some(Side::Watchlist) => watchlist_ref,
            None => play_ref,
        };
        // Once the window has the focus to give: a request while it is being made is lost.
        set_timeout(std::time::Duration::from_millis(if again.is_some() { 60 } else { 400 }), move || target.focus());
    }
    // The watchlist is Remux's: only a Remux server has it.
    let remux = resource(|| Some(()), |(), _| async { Ok::<_, String>(crate::hooks::server().is_remux().await) });

    let wide = is_wide();
    let large = wide || is_tv();
    let size = if large { ui::Size::Large } else { ui::Size::Regular };
    let open = {
        let media = media.clone();
        move || on_open(media.clone())
    };
    let toggle_favorite = {
        let media = media.clone();
        let server = server.clone();
        move || {
            PRESSED.with(|pressed| pressed.set(Some((media.id, Side::Favorite))));
            let now = !favorite.get_untracked();
            favorite.set(now);
            let (media, server) = (media.clone(), server.clone());
            spawn(async move {
                if let Err(error) = server.set_favorite(now, &media).await {
                    log::error!("favorite: {error}");
                    favorite.set(!now);
                }
            });
        }
    };
    let toggle_watched = {
        let media = media.clone();
        let server = server.clone();
        move || {
            PRESSED.with(|pressed| pressed.set(Some((media.id, Side::Watched))));
            let now = !watched.get_untracked();
            watched.set(now);
            let (media, server) = (media.clone(), server.clone());
            spawn(async move {
                if let Err(error) = server.set_watched(now, &media).await {
                    log::error!("watched: {error}");
                    watched.set(!now);
                }
            });
        }
    };
    let toggle_watchlist = {
        let media = media.clone();
        let server = server.clone();
        move || {
            PRESSED.with(|pressed| pressed.set(Some((media.id, Side::Watchlist))));
            let now = !watchlist.get_untracked();
            watchlist.set(now);
            let (media, server) = (media.clone(), server.clone());
            spawn(async move {
                if let Err(error) = server.set_watchlist(now, &media).await {
                    log::error!("watchlist: {error}");
                    watchlist.set(!now);
                }
            });
        }
    };
    // An episode's page leads to its show, at the episode's season (Remux's link over the title).
    let show_link = element_ref();
    let show = match (kind, &media.media_type, media.series_id, media.series_name.clone()) {
        (HeroKind::Page, MediaType::Episode, Some(series), Some(name)) => Some((series, name, media.season_id, media.season_number)),
        _ => None,
    };
    let has_show_link = show.is_some();
    let play_label = if media.progress().is_some() { "Resume" } else { "Play" };
    // A show plays its next episode, from its page (where it is known); else the title itself.
    let play = {
        let media = media.clone();
        move || {
            let media = media.clone();
            if media.is_series() {
                on_open(media);
            } else {
                crate::components::play_with_choice(media);
            }
        }
    };

    view()
        .size_full()
        .overflow_hidden()
        .child(
            image(ImageSrc::from(backdrop.unwrap_or_default()))
                .absolute()
                .inset_0()
                .object_fit(ResizeMode::Cover)
                .accessibility_elements_hidden(true),
        )
        // The picture and the title open the title's page (Remux's links over the backdrop): the
        // picture a layer of its own under the text, the text's empty room letting presses
        // through to it, so the buttons keep theirs (a press on a view around a button goes to
        // the view on the web). A TV goes by the buttons.
        .child((kind == HeroKind::Home && !is_tv()).then(|| {
            let open = open.clone();
            view().absolute().inset_0().test_id("hero-picture").accessibility_elements_hidden(true).on_press(open)
        }))
        // Fades into the page at the bottom, where the title and buttons stand.
        .child(
            view()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(relative(0.6))
                .pointer_events(PointerEvents::None)
                .bg_linear_to_t()
                .gradient_from(theme().background)
                .gradient_via(theme().background.opacity(0.6))
                .gradient_to(theme().background.opacity(0.0)),
        )
        // On a wide screen the text stands on the left, over a shade that keeps it readable.
        .child(wide.then(|| {
            view()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(relative(0.6))
                .pointer_events(PointerEvents::None)
                .bg_linear_to_r()
                .gradient_from(theme().background.opacity(0.7))
                .gradient_to(theme().background.opacity(0.0))
        }))
        .child(
            view()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .gap_4()
                .px(px(28.0))
                .pb(px(if wide { 56.0 } else { 16.0 }))
                .when(wide, |v| v.items_start().pl(px(if is_tv() { 72.0 } else { 24.0 })).max_w(px(768.0)))
                .when(!wide, |v| v.items_center())
                .pointer_events(PointerEvents::BoxNone)
                .child(show.map(|(series, name, season, number)| show_pill(show_link, series, name, season, number)))
                .child(
                    view()
                        .gap_2()
                        .when(kind == HeroKind::Home && !is_tv(), {
                            let open = open.clone();
                            move |v| v.on_press(open)
                        })
                        .when(wide, |v| v.items_start())
                        .when(!wide, |v| v.items_center())
                        .child(match logo {
                            Some(logo) => image(logo)
                                .h(px(if wide { 112.0 } else { 96.0 }))
                                .w(if wide { Length::from(px(288.0)) } else { relative(1.0).into() })
                                .object_fit(ResizeMode::Contain)
                                .when(wide, |i| i.object_left())
                                .label(media.title.clone())
                                .into_element(),
                            None => text(media.title.clone())
                                .text_4xl()
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme().foreground)
                                .when(!wide, |t| t.text_center())
                                .into_element(),
                        })
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_4()
                                .child(text(subtitle(&media)).text_sm().font_weight(FontWeight::MEDIUM).text_color(theme().foreground).number_of_lines(1))
                                .child(wide.then(|| ratings(&media))),
                        ),
                )
                .child(media.description.clone().filter(|_| wide).map(|overview| {
                    text(overview).max_w(px(448.0)).number_of_lines(3).text_size(px(15.0)).text_color(secondary_text())
                }))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        // On a TV room for the focused one's ring on either side of it, clear of the
                        // next button.
                        .gap(px(if is_tv() { 2.0 * crate::ui::FOCUS_RING_ROOM + 4.0 } else { 10.0 }))
                        // Up goes to the menu, down to the first row (Remux), not to the dots
                        // under the buttons that Android would find first.
                        .when(is_tv(), |v| {
                            v.capture_keys(tv_focus::capture(&["up", "down"])).on_key_down(move |event: &KeyEvent| match event.key.as_ref() {
                                "ArrowDown" => {
                                    focus.to_row_after(None);
                                }
                                // Up: the link to the show where there is one (a page over a place has
                                // no menu), else the menu.
                                "ArrowUp" => {
                                    if has_show_link {
                                        show_link.focus();
                                    } else {
                                        tv_focus::to_menu();
                                    }
                                }
                                _ => {}
                            })
                        })
                        // Play, with a part to cast with on its right where there is a Chromecast.
                        .child(
                            view()
                                .flex_row()
                                .child(
                                    ui::text_button(ui::Variant::Primary, size, Phosphor::PlayFill, play_label, play)
                                        .element_ref(&play_ref)
                                        .style_when(crate::components::can_cast, |s| s.rounded_tr(px(0.0)).rounded_br(px(0.0)).min_w(px(112.0))),
                                )
                                .child(crate::components::cast_part(if large { 44.0 } else { 40.0 })),
                        )
                        .child(ui::icon_button(
                            size,
                            move || if favorite.get() { Phosphor::HeartFill.src() } else { Phosphor::Heart.src() },
                            "Favorite",
                            toggle_favorite,
                        ).element_ref(&favorite_ref))
                        .child(ui::icon_button(
                            size,
                            move || if watched.get() { Phosphor::CheckCircleFill.src() } else { Phosphor::CheckCircle.src() },
                            "Watched",
                            toggle_watched,
                        ).element_ref(&watched_ref))
                        // The watchlist where there is room (a TV, a wide screen; a phone has it in
                        // More), on a Remux server.
                        .child(large.then(|| {
                            when(move || remux.get() == Some(true), move || {
                                ui::icon_button(
                                    size,
                                    move || if watchlist.get() { Phosphor::BookmarkSimpleFill.src() } else { Phosphor::BookmarkSimple.src() },
                                    "Watchlist",
                                    toggle_watchlist.clone(),
                                )
                                .element_ref(&watchlist_ref)
                            })
                        }))
                        .child((large && kind == HeroKind::Home).then(|| ui::icon_button(size, Phosphor::Info, "More info", open.clone())))
                        // A phone has the rest in a sheet (Remux's More).
                        .child((!large).then(|| {
                            let media = media.clone();
                            ui::icon_button(size, Phosphor::DotsThree, "More", move || crate::components::open_item_menu(media.clone()))
                        })),
                )
                .child((!wide).then(|| ratings(&media))),
        )
}

/// "The Simpsons · Season 38 ›" over an episode's title: its show's page, open at the season.
/// (Down from it is Android's own: the buttons under it.)
fn show_pill(me: ElementRef, series: uuid::Uuid, name: String, season: Option<uuid::Uuid>, number: Option<i64>) -> impl IntoElement {
    let label = match number {
        Some(number) => format!("{name} · Season {number}"),
        None => name,
    };
    nsh_base::button()
        .element_ref(&me)
        .self_center()
        .when(is_wide(), |b| b.self_start())
        .flex_row()
        .items_center()
        .gap_1()
        .px_3()
        .py(px(6.0))
        .rounded_full()
        .bg(crate::ui::tokens::scrim())
        .border_1()
        .border_color(theme().border)
        .accessibility_label(label.clone())
        .focus_visible(|s| s.bg(theme().primary).text_color(theme().primary_foreground))
        .text_color(theme().foreground)
        .on_press(move || {
            crate::tv_focus::pressed(me);
            if let Some(season) = season {
                crate::pages::detail::seasons::open_at(series, season);
            }
            crate::nav::push(crate::nav::Route::MediaDetail { media_type: MediaType::Series, id: series });
        })
        .child(text(label).text_sm().font_weight(FontWeight::SEMIBOLD).number_of_lines(1))
        .child(icon(Phosphor::CaretRight).size(px(14.0)))
}

/// "Movie · 2026 · Adventure · Horror": what the hero says under the title (Remux's `subtitle`).
fn subtitle(media: &Media) -> String {
    use chrono::Datelike;
    let mut parts = vec![media.media_type.to_string()];
    parts.extend(media.release_date.map(|date| date.year().to_string()));
    parts.extend(media.genres.iter().cloned());
    parts.join(" · ")
}

/// The critics' and the audience's scores (Remux's `RatingsDisplay`).
fn ratings(media: &Media) -> impl IntoElement {
    view().flex_row().items_center().gap_4().children(media.ratings.iter().map(|rating| {
        let value = rating.format_score();
        let (label, glyph) = match rating.source {
            RatingSource::RottenTomatoes => (format!("Rotten Tomatoes {value}%"), None),
            RatingSource::Audience => (format!("Audience {value}"), Some(Phosphor::StarFill)),
        };
        view()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .label(label)
            .child(rating.icon().map(|logo| image(logo).h(px(16.0)).w(px(16.0)).object_fit(ResizeMode::Contain)))
            .child(glyph.map(|glyph| icon(glyph).size(px(16.0)).text_color(rating_star())))
            .child(text(value).text_size(px(15.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme().foreground))
    }))
}
