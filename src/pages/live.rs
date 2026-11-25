//! Live TV (Remux's `LiveTvView`, `LiveChannelsView`, `LiveGuideView`, `ProgramCard`): what is on
//! now and what is coming by kind, the channels, and the guide. Watching comes with the player
//! (PLAN.md, step 3): a programme pressed shows what it is.

use chrono::{DurationRound, Utc};
use nsh_base::{theme};
use nsh_core::prelude::*;
use nsh_icon::icon;
use nsh_icon_phosphor::Phosphor;

use crate::components::{media_grid, page_header};
use crate::hooks::server;
use crate::media::{MediaType, Program, ProgramKind};
use crate::nav::{self, Route};
use crate::server::MediaQuery;
use crate::ui::{self, tokens::{group, is_tv, is_wide, live, motion, track}};

/// A page without the system's header.
fn page(content: impl IntoElement) -> AnyElement {
    crate::stack::page(view().flex_1().bg(theme().background).child(content))
}

/// A page opened over Live TV: Back with a swipe from the left edge.
fn page_over(content: impl IntoElement) -> AnyElement {
    page(content)
}

fn inset() -> f32 {
    if is_wide() { 24.0 } else { 16.0 }
}

/// The programme whose details are shown, if any.
#[derive(Clone, Default, PartialEq)]
struct Shown(Option<Program>);

/// A programme: its channel's logo with how far it is and how long is left, the channel, the
/// title and the time.
fn program_card(program: Program) -> impl IntoElement {
    let now = Utc::now();
    let airing = program.airing(now);
    let logo = server().channel_logo(program.channel_id);
    let width = if is_tv() { 320.0 } else { 240.0 };
    let shown = global::<Shown>();
    view()
        .focusable(true)
        .role(AccessibilityRole::Button)
        .label(format!("{}, {}", program.channel_name, program.title))
        .w(px(width))
        .gap_1()
        .transition_transform()
        .duration(motion::FAST)
        .active(|s| s.scale(motion::PRESS_SCALE))
        .focus_visible(|s| s.scale(if is_tv() { 1.05 } else { 1.0 }))
        .on_press({
            let program = program.clone();
            move || shown.set(Shown(Some(program.clone())))
        })
        .child(
            view()
                .w_full()
                .aspect_ratio(16.0 / 9.0)
                .rounded_xl()
                .overflow_hidden()
                .bg(group())
                .items_center()
                .justify_center()
                .child(icon(Phosphor::Television).size(px(56.0)).text_color(theme().muted_foreground).absolute())
                .child(image(logo).absolute().inset_0().m_8().object_fit(ResizeMode::Contain))
                .child(airing.then(|| {
                    view()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .px_3()
                        .pb_3()
                        .gap(px(6.0))
                        .child(text(format!("{}m", program.minutes_left(now))).text_lg().font_weight(FontWeight::SEMIBOLD).text_color(theme().foreground))
                        .child(
                            view()
                                .h(px(6.0))
                                .rounded_full()
                                .overflow_hidden()
                                .bg(track())
                                .child(view().h_full().rounded_full().w(nsh_core::geometry::relative(program.progress(now) as f32 / 100.0)).bg(live())),
                        )
                })),
        )
        .child(text(program.channel_name.clone()).mt_1().text_lg().font_weight(FontWeight::SEMIBOLD).number_of_lines(1).text_color(theme().foreground))
        .child(text(program.title.clone()).text_base().number_of_lines(1).text_color(theme().foreground))
        .child(text(program.time_range()).text_base().text_color(theme().muted_foreground))
}

/// What a pressed programme is: its channel and time, its episode and story.
fn program_sheet() -> impl IntoElement {
    let shown = global::<Shown>();
    let open = signal(false);
    effect(move || open.set(shown.with(|shown| shown.0.is_some())));
    effect(move || {
        if !open.get() {
            untracked(|| shown.set(Shown(None)));
        }
    });
    dynamic(move || match shown.with(|shown| shown.0.clone()) {
        None => view().into_element(),
        Some(program) => ui::sheet(open, program.title.clone(), move || {
            view()
                .gap_3()
                .px_1()
                .child(text(format!("{} · {}", program.channel_name, program.time_range())).text_size(px(15.0)).text_color(theme().muted_foreground))
                .child(program.episode_title.clone().map(|episode| text(episode).text_size(px(17.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme().foreground)))
                .child(program.overview.clone().map(|story| text(story).text_size(px(15.0)).text_color(theme().foreground)))
                .child(ui::menu_note("Watching comes with the player."))
        })
        .into_element(),
    })
}

/// A titled row of programmes.
fn program_row(title: String, load: impl Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Program>, String>>>> + 'static) -> impl IntoElement {
    let programs = resource(|| Some(()), move |(), _| load());
    view()
        .gap_2()
        // Nothing on of a kind: no row.
        .style_when(move || programs.get().is_some_and(|list| list.is_empty()), |s| s.hidden())
        .child(text(title).px(px(inset())).text_xl().font_weight(FontWeight::SEMIBOLD).text_color(theme().foreground))
        .child(dynamic(move || match (programs.get(), programs.error()) {
            (_, Some(error)) => ui::menu_note(error).into_element(),
            (None, _) => view().h(px(160.0)).items_center().justify_center().child(ui::spinner()).into_element(),
            (Some(list), _) => scroll()
                .horizontal()
                .scrollbar(false)
                .child(view().flex_row().gap_4().px(px(inset())).py_2().children(list.into_iter().map(program_card)))
                .into_element(),
        }))
}

/// The Live TV place.
pub fn live_tv() -> AnyElement {
    let scroll_ref = element_ref();
    page(
        scroll()
            .element_ref(&scroll_ref)
            .on_scroll(crate::tv_focus::root_scroll(crate::nav::Place::LiveTv, scroll_ref))
            .flex_1()
            .child(
                view()
                    .pb(crate::ui::tokens::bottom_room(140.0))
                    .gap_6()
                    .child(
                        view().px(px(inset())).gap_3().child(page_header("Live TV", false)).child(ui::menu_group(vec![
                            ui::menu_row(Some(Phosphor::Television), "Channels", true, || nav::push(Route::LiveChannels)),
                            ui::menu_row(Some(Phosphor::CalendarBlank), "Guide", true, || nav::push(Route::LiveGuide)),
                        ])),
                    )
                    .child(program_row("On Now".into(), || Box::pin(async { server().programs_on_now(30).await.map_err(|error| error.to_string()) })))
                    .children(ProgramKind::ALL.into_iter().map(|kind| {
                        program_row(kind.label().to_owned(), move || {
                            Box::pin(async move { server().programs_of_kind(kind, 20).await.map_err(|error| error.to_string()) })
                        })
                    })),
            )
            .child(program_sheet()),
    )
}

/// The channels, as a grid.
pub fn channels() -> AnyElement {
    page_over(media_grid(page_header("Channels", true), MediaQuery::builder().types(vec![MediaType::Channel]).build(), true))
}

/// Points for a minute of the guide.
const PER_MINUTE: f32 = 5.0;
/// The column of the channels.
const CHANNEL_WIDTH: f32 = 120.0;
/// How far ahead the guide goes.
const GUIDE_HOURS: i64 = 6;
/// How many channels it shows.
const CHANNELS: u32 = 30;

/// The guide: a line for each channel, its programmes by their time, from half an hour ago.
pub fn guide() -> AnyElement {
    let now = Utc::now();
    let from = now.duration_trunc(chrono::Duration::minutes(30)).unwrap_or(now) - chrono::Duration::minutes(30);
    let to = from + chrono::Duration::hours(GUIDE_HOURS);
    let total = GUIDE_HOURS as f32 * 60.0 * PER_MINUTE;
    let data = resource(|| Some(()), move |(), _| async move {
        let server = server();
        let query = MediaQuery { limit: CHANNELS, ..MediaQuery::builder().types(vec![MediaType::Channel]).build() };
        let channels = server.media(&query).await.map_err(|error| error.to_string())?;
        let ids: Vec<_> = channels.iter().map(|channel| channel.id).collect();
        let programs = server.programs_between(&ids, from, to).await.unwrap_or_default();
        Ok::<_, String>((channels, programs))
    });
    let at = move |time: chrono::DateTime<Utc>| ((time - from).num_seconds() as f32 / 60.0 * PER_MINUTE).clamp(0.0, total);
    page_over(view().flex_1().child(view().px(px(inset())).child(page_header("Guide", true))).child(dynamic(move || {
        let Some((channels, programs)) = data.get() else {
            return match data.error() {
                Some(error) => ui::empty(error).into_element(),
                None => view().flex_1().items_center().justify_center().child(ui::spinner()).into_element(),
            };
        };
        let shown = global::<Shown>();
        let hours = (0..GUIDE_HOURS * 2).map(move |step| {
            let time = from + chrono::Duration::minutes(step * 30);
            text(time.with_timezone(&chrono::Local).format("%H:%M").to_string())
                .absolute()
                .left(px(CHANNEL_WIDTH + at(time) + 4.0))
                .text_xs()
                .text_color(theme().muted_foreground)
        });
        let lines = channels.into_iter().map(move |channel| {
            let logo = server().channel_logo(channel.id);
            let own: Vec<Program> = programs.iter().filter(|program| program.channel_id == channel.id).cloned().collect();
            view()
                .h(px(64.0))
                .flex_row()
                .child(
                    view()
                        .w(px(CHANNEL_WIDTH))
                        .h_full()
                        .p_2()
                        .items_center()
                        .justify_center()
                        .child(image(logo).w_full().h_full().object_fit(ResizeMode::Contain).label(channel.title.clone())),
                )
                .child(view().w(px(total)).h_full().children(own.into_iter().map(move |program| {
                    let left = at(program.start);
                    let width = (at(program.end) - left - 4.0).max(24.0);
                    let airing = program.airing(Utc::now());
                    view()
                        .focusable(true)
                        .role(AccessibilityRole::Button)
                        .label(program.title.clone())
                        .absolute()
                        .top(px(4.0))
                        .bottom(px(4.0))
                        .left(px(left))
                        .w(px(width))
                        .px_2()
                        .justify_center()
                        .rounded_md()
                        .bg(if airing { theme().foreground.opacity(0.15) } else { group() })
                        .active(|s| s.bg(theme().accent))
                        .focus_visible(|s| s.bg(theme().primary))
                        .on_press({
                            let program = program.clone();
                            move || shown.set(Shown(Some(program.clone())))
                        })
                        .child(text(program.title.clone()).text_sm().font_weight(FontWeight::MEDIUM).number_of_lines(1).text_color(theme().foreground))
                        .child(text(program.time_range()).text_xs().number_of_lines(1).text_color(theme().muted_foreground))
                })))
        });
        let now_x = CHANNEL_WIDTH + at(Utc::now());
        view()
            .flex_1()
            .child(
                scroll().horizontal().flex_1().child(
                    view()
                        .w(px(CHANNEL_WIDTH + total))
                        .child(view().h(px(24.0)).children(hours))
                        .child(scroll().flex_1().child(view().pb(crate::ui::tokens::bottom_room(140.0)).children(lines)))
                        // Now.
                        .child(view().absolute().top_0().bottom_0().left(px(now_x)).w(px(2.0)).bg(live())),
                ),
            )
            .child(program_sheet())
            .into_element()
    })))
}
