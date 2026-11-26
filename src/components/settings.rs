//! The settings, in a sheet opened from the avatar (Remux's `SettingsSheet`): pages one inside the
//! other, the way back to the one around it at the top.

use nsh_base::theme;
use nsh_core::prelude::*;
use nsh_icon_phosphor::Phosphor;

use crate::hooks::{self, server};
use crate::media::Library;
use crate::session;
use crate::ui::{self, FieldKind, Size, Variant};

/// The pages of the sheet.
#[derive(Clone, Copy, PartialEq, Default)]
enum Page {
    #[default]
    Main,
    Account,
    QuickConnect,
    Password,
    Libraries,
    Playback,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Page::Main => "Settings",
            Page::Account => "User",
            Page::QuickConnect => "Quick Connect",
            Page::Password => "Password",
            Page::Libraries => "Library order",
            Page::Playback => "Playback",
        }
    }

    fn parent(self) -> Option<Page> {
        match self {
            Page::Main => None,
            Page::Account | Page::Libraries | Page::Playback => Some(Page::Main),
            Page::QuickConnect | Page::Password => Some(Page::Account),
        }
    }
}

/// Whether the sheet is open, and at which page.
#[derive(Clone, Copy, PartialEq, Default)]
struct Settings {
    open: bool,
    page: Page,
}

fn state() -> Signal<Settings> {
    global::<Settings>()
}

/// Opens the settings at their first page.
pub fn open_settings() {
    state().set(Settings { open: true, page: Page::Main });
}

fn go(page: Page) {
    state().update(|state| state.page = page);
}

fn close() {
    state().update(|state| state.open = false);
}

/// The settings sheet, once for the app: in the shell.
pub fn settings_host() -> impl IntoElement {
    let open = signal(false);
    effect(move || open.set(state().with(|state| state.open)));
    effect(move || {
        let shown = open.get();
        untracked(|| {
            if !shown && state().with(|state| state.open) {
                state().update(|state| state.open = false);
            }
        });
    });
    ui::sheet(open, "Settings", move || {
        dynamic(move || {
            let page = state().with(|state| state.page);
            untracked(move || {
                view()
                    .gap_4()
                    .child(page.parent().map(|parent| {
                        ui::menu_group(vec![ui::menu_row(Some(Phosphor::CaretLeft), format!("Back to {}", parent.title()), false, move || go(parent))])
                    }))
                    .child(page.parent().map(|_| ui::menu_heading(page.title())))
                    .child(match page {
                        Page::Main => main_page().into_element(),
                        Page::Account => account_page().into_element(),
                        Page::QuickConnect => quick_connect_page().into_element(),
                        Page::Password => password_page().into_element(),
                        Page::Libraries => libraries_page().into_element(),
                        Page::Playback => playback_page().into_element(),
                    })
                    .into_element()
            })
        })
    })
}

/// What happened to the last thing asked for: shown under it.
fn outcome(message: Signal<Option<String>>) -> impl IntoElement {
    when(move || message.get().is_some(), move || ui::menu_note(message.get().unwrap_or_default()))
}

fn main_page() -> impl IntoElement {
    let remux = resource(|| Some(()), |(), _| async { Ok::<_, String>(server().is_remux().await) });
    let admin = resource(|| Some(()), |(), _| async {
        let server = server();
        Ok::<_, String>(server.is_remux().await && server.is_admin().await)
    });
    let server = server();
    let username = server.username.clone();
    let admin_url = format!("{}/web/#/dashboard", server.address());
    let accounts: Vec<_> = session::accounts().into_iter().filter(|stored| !stored.is_account_of(&server.to_stored())).collect();
    view()
        .gap_4()
        .child(ui::menu_group(vec![ui::menu_row(Some(Phosphor::UserCircle), if username.is_empty() { "User".to_owned() } else { username }, true, || go(Page::Account))]))
        // The dashboard of a Remux server, for its administrators: the system's browser opens it.
        .child(when(
            move || admin.get().unwrap_or(false),
            move || {
                let url = admin_url.clone();
                ui::menu_group(vec![ui::menu_row(Some(Phosphor::Gauge), "Admin dashboard", true, move || {
                    let _ = open_url(url.clone());
                })])
            },
        ))
        .child(dynamic(move || {
            let mut rows = Vec::new();
            // Playback has what only a Remux server does something with (the HDR switch comes with
            // the player).
            if remux.get().unwrap_or(false) || crate::capabilities::display_hdr().screen_shows_hdr() {
                rows.push(ui::menu_row(None, "Playback", true, || go(Page::Playback)));
            }
            rows.push(ui::menu_row(None, "Library order", true, || go(Page::Libraries)));
            ui::menu_group(rows).into_element()
        }))
        // The other accounts of this device, and a way to sign in as one that is not there yet.
        .child(ui::menu_heading("Accounts"))
        .child(ui::menu_group(
            accounts
                .into_iter()
                .map(|stored| {
                    let label = format!("{} · {}", stored.username, stored.name);
                    ui::menu_row(Some(Phosphor::UserCircle), label, true, move || {
                        let stored = stored.clone();
                        spawn(async move {
                            if let Ok(Some(server)) = crate::server::JellyfinServer::resume(stored).await {
                                close();
                                hooks::set_session(server);
                            }
                        });
                    })
                })
                .chain(std::iter::once(ui::menu_row(Some(Phosphor::Plus), "Add account", false, || {
                    close();
                    hooks::sign_out();
                })))
                .collect(),
        ))
        .child(ui::menu_note("Accounts that you signed in with stay here: sign in to one again without a password."))
        // The web has the browser's console instead.
        .child((!cfg!(target_arch = "wasm32")).then(log_row))
        .child(ui::text_button(Variant::Primary, Size::Regular, Phosphor::SignOut, "Log out", || {
            close();
            // The account stays in the list, with its token, to sign in to again.
            hooks::sign_out();
        }))
        // Which build this is, for whoever has to say.
        .child(
            text(format!("Remux {} · NSH", crate::app_host::app_host().remux_version))
                .pt_2()
                .text_center()
                .text_size(px(13.0))
                .text_color(theme().muted_foreground),
        )
}

/// Sends the log of this run to the server, where someone can read it.
fn log_row() -> impl IntoElement {
    let message = signal(None::<String>);
    let busy = stored(false);
    view()
        .gap_2()
        .child(ui::menu_group(vec![ui::menu_row(Some(Phosphor::PaperPlaneTilt), "Send log to server", false, move || {
            if busy.with(|busy| *busy) {
                return;
            }
            busy.with(|busy| *busy = true);
            message.set(Some("Sending the log…".to_owned()));
            spawn(async move {
                let text = crate::log_text::text().await;
                let answer = server().upload_log(text).await;
                log::info!("send log: {:?}", answer.as_ref().map_err(|error| error.to_string()));
                busy.with(|busy| *busy = false);
                message.set(Some(match answer {
                    Ok(name) => format!("Sent to the server as {name}"),
                    Err(error) => format!("Could not send the log: {error}"),
                }));
            });
        })]))
        .child(outcome(message))
}

fn account_page() -> impl IntoElement {
    ui::menu_group(vec![
        ui::menu_row(None, "Quick Connect", true, || go(Page::QuickConnect)),
        ui::menu_row(None, "Password", true, || go(Page::Password)),
    ])
}

/// Signs another device in as this user, with the code it shows.
fn quick_connect_page() -> impl IntoElement {
    let code = signal(String::new());
    let message = signal(None::<String>);
    let authorize = move || {
        let wanted = code.get_untracked().trim().to_owned();
        if wanted.is_empty() {
            return;
        }
        spawn(async move {
            match server().authorize_quick_connect(&wanted).await {
                Ok(()) => {
                    code.set(String::new());
                    message.set(Some("The other device is signed in.".into()));
                }
                Err(error) => message.set(Some(error.to_string())),
            }
        });
    };
    view()
        .gap_3()
        .child(ui::menu_note("This user will be authenticated to the other device."))
        .child(ui::field(code, "Quick Connect code", move |_| authorize()))
        .child(ui::text_button(Variant::Primary, Size::Regular, Phosphor::Check, "Authorize", authorize))
        .child(outcome(message))
}

/// Changes the password; the server signs the user out everywhere afterwards.
fn password_page() -> impl IntoElement {
    let current = signal(String::new());
    let new = signal(String::new());
    let confirm = signal(String::new());
    let message = signal(None::<String>);
    let save = move || {
        let (old, wanted, again) = (current.get_untracked(), new.get_untracked(), confirm.get_untracked());
        if wanted.is_empty() || wanted != again {
            message.set(Some("The passwords do not match.".into()));
            return;
        }
        spawn(async move {
            match server().change_password(&old, &wanted).await {
                Ok(()) => {
                    close();
                    hooks::sign_out();
                }
                Err(error) => message.set(Some(error.to_string())),
            }
        });
    };
    view()
        .gap_3()
        .child(ui::field_of(FieldKind::Password, current, "Current password", |_| {}))
        .child(ui::field_of(FieldKind::Password, new, "New password", |_| {}))
        .child(ui::field_of(FieldKind::Password, confirm, "Confirm new password", move |_| save()))
        .child(ui::menu_note("Changes the password of your account on the server. You are signed out afterwards, on every device."))
        .child(ui::text_button(Variant::Primary, Size::Regular, Phosphor::Check, "Change password", save))
        .child(outcome(message))
}

/// The order of the libraries on Home and in Media: a library moved up or down is saved at once.
/// (Remux drags a row; buttons work with a remote and a screen reader too.)
fn libraries_page() -> impl IntoElement {
    let libraries = signal(Vec::<Library>::new());
    let forced = signal(false);
    let message = signal(None::<String>);
    spawn(async move {
        let server = server();
        forced.set(server.library_order_forced().await);
        match server.libraries().await {
            Ok(list) => libraries.set(list.into_iter().filter(|library| !library.live_tv).collect()),
            Err(error) => message.set(Some(format!("Could not load the libraries: {error}"))),
        }
    });
    let shift = move |from: usize, to: usize| {
        let before = libraries.get_untracked();
        let mut list = before.clone();
        let moved = list.remove(from);
        list.insert(to, moved);
        libraries.set(list.clone());
        spawn(async move {
            let order: Vec<_> = list.iter().map(|library| library.id).collect();
            match server().set_library_order(&order).await {
                Ok(true) => message.set(None),
                // The server decides: what it shows is what stays.
                Ok(false) => {
                    forced.set(true);
                    libraries.set(before);
                }
                Err(error) => {
                    libraries.set(before);
                    message.set(Some(format!("Could not save the order: {error}")));
                }
            }
        });
    };
    view()
        .gap_3()
        .child(when(move || forced.get(), || ui::menu_note("The order of the libraries is set by the server, for everyone.")))
        .child(outcome(message))
        .child(dynamic(move || {
            let list = libraries.get();
            let last = list.len().saturating_sub(1);
            let locked = forced.get();
            ui::menu_group(
                list.into_iter()
                    .enumerate()
                    .map(|(at, library)| {
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .px_4()
                            .py_2()
                            .min_h(px(56.0))
                            .child(text(library.name).flex_1().text_size(px(17.0)).number_of_lines(1).text_color(theme().foreground))
                            .child((!locked && at > 0).then(|| ui::icon_button(Size::Regular, Phosphor::CaretUp, "Move up", move || shift(at, at - 1))))
                            .child((!locked && at < last).then(|| ui::icon_button(Size::Regular, Phosphor::CaretDown, "Move down", move || shift(at, at + 1))))
                            .into_element()
                    })
                    .collect(),
            )
            .into_element()
        }))
}

fn playback_page() -> impl IntoElement {
    let remux = resource(|| Some(()), |(), _| async { Ok::<_, String>(server().is_remux().await) });
    let hdr = crate::capabilities::display_hdr();
    view()
        .gap_3()
        // HDR as HDR, where the screen shows it.
        .child(hdr.screen_shows_hdr().then(|| {
            view()
                .gap_3()
                .child(ui::menu_group(vec![ui::switch_row(
                    "HDR",
                    || !crate::settings::settings().hdr_off,
                    |on| crate::settings::update(|settings| settings.hdr_off = !on),
                )]))
                .child(ui::menu_note("Off, the server makes HDR titles SDR for this device."))
        }))
        .child(when(
            move || remux.get().unwrap_or(false),
            || {
                view()
                    .gap_3()
                    .child(ui::menu_group(vec![ui::switch_row(
                        "Automatic quality",
                        || crate::settings::settings().auto_quality,
                        |on| crate::settings::update(|settings| settings.auto_quality = on),
                    )]))
                    .child(ui::menu_note("Picks the versions of a title that fit the connection, by the speed measured to the server."))
                    // What was measured, and measuring again (Remux's).
                    .child(ui::menu_group(vec![ui::menu_row(
                        Some(Phosphor::Gauge),
                        move || {
                            SharedString::from(if crate::profile::measuring() {
                                "Measuring…".to_owned()
                            } else {
                                "Measure again".to_owned()
                            })
                        },
                        false,
                        || {
                            if !crate::profile::measuring() {
                                crate::profile::retest();
                            }
                        },
                    )]))
                    .child(ui::menu_note(move || {
                        SharedString::from(match (crate::profile::speed(), crate::profile::latency()) {
                            (Some(bits), Some(ms)) => format!("Connection to the server: {:.0} Mbit/s · {ms} ms", bits as f64 / 1_000_000.0),
                            (Some(bits), None) => format!("Connection to the server: {:.0} Mbit/s", bits as f64 / 1_000_000.0),
                            _ => "Connection to the server: not measured yet".to_owned(),
                        })
                    }))
            },
        ))
        .child(text(hdr.describe()).text_center().text_size(px(13.0)).text_color(theme().muted_foreground))
}
