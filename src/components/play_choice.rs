//! What Play asks before it plays (Remux's `PlayButton`): which version of a film or episode,
//! also when there is one (the viewer sees what plays); and, where there are Chromecasts, a part
//! of its own to cast with (Remux's `CastSheet`).

use nsh_base::{button, theme};
use nsh_core::prelude::*;
use nsh_icon::labeled_icon;
use nsh_icon_phosphor::Phosphor;

use crate::media::{Media, MediaSource, MediaType};
use crate::ui::{self, tokens::{is_tv, motion}};

/// The title whose versions are asked about, and its versions once they are in.
#[derive(Clone, Default, PartialEq)]
struct Choosing(Option<(Media, Option<Vec<MediaSource>>)>);

/// Whether the sheet of Chromecasts is open.
#[derive(Clone, Copy, Default, PartialEq)]
struct CastOpen(bool);

/// Plays `media`: a film or an episode after its version is chosen, anything else at once.
pub fn play_with_choice(media: Media) {
    if !matches!(media.media_type, MediaType::Movie | MediaType::Episode) {
        crate::pages::player::play(media);
        return;
    }
    let choosing = global::<Choosing>();
    choosing.set(Choosing(Some((media.clone(), None))));
    spawn(async move {
        // The device cast to is asked as itself: the server sorts the versions by what it plays.
        let server = untracked(crate::cast::playback_server).unwrap_or_else(crate::hooks::server);
        let versions = server.media_sources(media.id).await.unwrap_or_default();
        // Closed meanwhile, or another title asked about.
        if choosing.with_untracked(|choosing| choosing.0.as_ref().map(|(m, _)| m.id)) != Some(media.id) {
            return;
        }
        if versions.is_empty() {
            choosing.set(Choosing(None));
            crate::pages::player::play(media);
        } else {
            choosing.set(Choosing(Some((media, Some(versions)))));
        }
    });
}

/// Opens the sheet of Chromecasts.
pub fn open_cast_sheet() {
    global::<CastOpen>().set(CastOpen(true));
}

/// Whether there is a Chromecast to cast to (or one cast to): Play has its cast part then.
pub fn can_cast() -> bool {
    !is_tv() && crate::cast::hub().with(|hub| !hub.devices.is_empty() || hub.active.is_some())
}

/// The sheets, once for the app: in the shell.
pub fn play_choice_host() -> impl IntoElement {
    let choosing = global::<Choosing>();
    let versions_open = signal(false);
    effect(move || versions_open.set(choosing.with(|choosing| choosing.0.is_some())));
    effect(move || {
        if !versions_open.get() {
            untracked(|| {
                if choosing.with_untracked(|choosing| choosing.0.is_some()) {
                    choosing.set(Choosing(None));
                }
            });
        }
    });
    let cast_open = signal(false);
    effect(move || cast_open.set(global::<CastOpen>().get().0));
    effect(move || {
        if !cast_open.get() {
            untracked(|| global::<CastOpen>().set(CastOpen(false)));
        }
    });
    view()
        .child(ui::sheet(versions_open, "Version", move || {
            let Some((media, versions)) = choosing.with(|choosing| choosing.0.clone()) else {
                return view().into_element();
            };
            match versions {
                None => view().items_center().py_6().child(ui::spinner()).into_element(),
                Some(versions) => {
                    // A TV's remote on the first version, once they are there (the sheet put it
                    // on its close button while they loaded, and the page under it took it back).
                    let first = element_ref();
                    if crate::ui::tokens::is_tv() {
                        set_timeout(std::time::Duration::from_millis(150), move || first.focus());
                    }
                    ui::menu_group(
                        versions
                            .into_iter()
                            .enumerate()
                            .map(|(at, source)| {
                                let (title, detail) = (source.title(), source.subtitle().filter(|detail| *detail != source.title()));
                                let media = media.clone();
                                ui::menu_choice_at((at == 0).then_some(first), title, detail, false, move || {
                                    choosing.set(Choosing(None));
                                    crate::pages::player::play_source(media.clone(), source.clone());
                                })
                            })
                            .collect(),
                    )
                    .into_element()
                }
            }
        }))
        .child(ui::sheet(cast_open, "Cast to", move || {
            let (devices, active) = crate::cast::hub().with(|hub| (hub.devices.clone(), hub.active.clone()));
            let mut content = view().gap_3();
            content = if devices.is_empty() {
                content.child(ui::menu_note("Looking for Chromecasts… Make sure one is on, and on the same network as this device."))
            } else {
                content.child(ui::menu_group(
                    devices
                        .into_iter()
                        .map(|device| {
                            let on = active.as_ref().map(|active| &active.id) == Some(&device.id);
                            let (name, model) = (device.name.clone(), Some(device.model.clone()).filter(|model| !model.is_empty()));
                            ui::menu_choice_checked(name, model, on, move || {
                                cast_open.set(false);
                                crate::cast::pick(device.clone());
                            })
                        })
                        .collect(),
                ))
            };
            if active.is_some() {
                content = content.child(ui::menu_group(vec![ui::menu_row(Some(Phosphor::X), "Stop casting", false, move || {
                    cast_open.set(false);
                    crate::cast::stop();
                })]));
            }
            content.into_element()
        }))
}

/// The cast part of Play: a round end to its right, lit while casting.
pub fn cast_part(height: f32) -> impl IntoElement {
    when(can_cast, move || {
        button()
            .h(px(height))
            .w(px(48.0))
            .items_center()
            .justify_center()
            .rounded_r(px(height / 2.0))
            .bg(theme().secondary)
            .text_color(move || if crate::cast::active().is_some() { theme().primary } else { theme().secondary_foreground })
            .accessibility_label("Cast to a Chromecast")
            .transition_transform()
            .duration(motion::FAST)
            .active(|s| s.opacity(0.8))
            .on_press(open_cast_sheet)
            .child(labeled_icon(move || if crate::cast::active().is_some() { Phosphor::ScreencastFill.src() } else { Phosphor::Screencast.src() }, "Cast").size(px(20.0)))
    })
}
