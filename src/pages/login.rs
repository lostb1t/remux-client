//! Signing in (Remux's `LoginView`): a server, a user and a password; or Quick Connect, a code
//! another device that is signed in approves; or an account that was signed in on this device.

use std::time::Duration;

use nsh_base::theme;
use nsh_core::prelude::*;
use nsh_icon_phosphor::Phosphor;

use crate::hooks::set_session;
use crate::server::{JellyfinServer, QuickConnectRequest};
use crate::session;
use crate::ui::{self, FieldKind, Size, Variant};

/// The login screen, over the whole window.
pub fn login() -> impl IntoElement {
    let host = signal(String::new());
    let username = signal(String::new());
    let password = signal(String::new());
    let error = signal(None::<String>);
    let loading = signal(false);
    let code = signal(None::<String>);
    // A Quick Connect that was started, so a new one or Cancel ends the one before.
    let attempt = stored(0u32);

    let log_in = move || {
        let (host, username, password) = (host.get_untracked(), username.get_untracked(), password.get_untracked());
        if host.trim().is_empty() || username.trim().is_empty() {
            error.set(Some("Please fill in the server and the username.".into()));
            return;
        }
        loading.set(true);
        error.set(None);
        spawn(async move {
            match JellyfinServer::from_credentials(&host, &username, &password).await {
                Ok(server) => set_session(server),
                Err(failure) => {
                    loading.set(false);
                    error.set(Some(failure.to_string()));
                }
            }
        });
    };

    // Asks for a code, then for the session until the other device has approved it.
    let quick_connect = move || {
        let host = host.get_untracked();
        if host.trim().is_empty() {
            error.set(Some("Please fill in the server first.".into()));
            return;
        }
        error.set(None);
        let this = attempt.with(|attempt| {
            *attempt += 1;
            *attempt
        });
        spawn(async move {
            let request: QuickConnectRequest = match JellyfinServer::start_quick_connect(&host).await {
                Ok(request) => request,
                Err(failure) => {
                    error.set(Some(failure.to_string()));
                    return;
                }
            };
            code.set(Some(request.code.clone()));
            // A code is good for a few minutes.
            for _ in 0..150 {
                timer(Duration::from_secs(2)).await;
                if attempt.with(|attempt| *attempt) != this {
                    return;
                }
                match JellyfinServer::finish_quick_connect(&request).await {
                    Ok(Some(server)) => {
                        code.set(None);
                        set_session(server);
                        return;
                    }
                    Ok(None) => {}
                    Err(failure) => {
                        code.set(None);
                        error.set(Some(failure.to_string()));
                        return;
                    }
                }
            }
            code.set(None);
            error.set(Some("The Quick Connect code expired. Try again.".into()));
        });
    };
    let cancel = move || {
        attempt.with(|attempt| *attempt += 1);
        code.set(None);
    };

    let accounts = session::accounts();
    scroll().flex_1().bg(theme().background).child(
        view()
            .w_full()
            .max_w(px(420.0))
            .self_center()
            .px_6()
            .pt_safe_area_plus(px(64.0))
            .pb(px(48.0))
            .gap_4()
            .child(text("Sign in").text_2xl().font_weight(FontWeight::BOLD).text_center().text_color(theme().foreground))
            .child(dynamic(move || match code.get() {
                // Waiting for the other device: the code, big, and a way out.
                Some(code) => view()
                    .gap_4()
                    .items_center()
                    .child(ui::menu_note("On a device where you are signed in to Remux, open Settings, User, Quick Connect and enter this code."))
                    .child(text(code).text_4xl().font_weight(FontWeight::BOLD).letter_spacing(px(6.0)).text_color(theme().foreground))
                    .child(ui::text_button(Variant::Secondary, Size::Regular, Phosphor::X, "Cancel", cancel))
                    .into_element(),
                None => view()
                    .gap_3()
                    .child(ui::field_of(FieldKind::Url, host, "Server URL", |_| {}))
                    .child(ui::field_of(FieldKind::Username, username, "Username", |_| {}))
                    .child(ui::field_of(FieldKind::Password, password, "Password", move |_| log_in()))
                    .child(
                        ui::text_button(Variant::Primary, Size::Large, Phosphor::SignIn, "Log In", log_in)
                            .disabled(move || loading.get())
                            .mt_2(),
                    )
                    .child(ui::text_button(Variant::Secondary, Size::Regular, Phosphor::QrCode, "Sign in with Quick Connect", quick_connect))
                    .into_element(),
            }))
            .child(when(move || error.get().is_some(), move || ui::menu_note(error.get().unwrap_or_default())))
            // Accounts that were signed in on this device: back in without a password.
            .child((!accounts.is_empty()).then(|| {
                view().pt_6().gap_2().child(ui::menu_heading("Accounts on this device")).child(ui::menu_group(
                    accounts
                        .into_iter()
                        .map(|stored| {
                            let label = format!("{} · {}", if stored.username.is_empty() { "?" } else { &stored.username }, stored.name);
                            ui::menu_row(Some(Phosphor::UserCircle), label, true, move || {
                                let stored = stored.clone();
                                loading.set(true);
                                spawn(async move {
                                    match JellyfinServer::resume(stored).await {
                                        Ok(Some(server)) => set_session(server),
                                        Ok(None) => {
                                            loading.set(false);
                                            error.set(Some("That account is signed out. Sign in again.".into()));
                                        }
                                        Err(failure) => {
                                            loading.set(false);
                                            error.set(Some(failure.to_string()));
                                        }
                                    }
                                });
                            })
                        })
                        .collect(),
                ))
            })),
    )
}
