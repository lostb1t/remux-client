//! Remux, as a Native Shell app: a proof of concept of its home screen (see `DESIGN.md`).

mod app_host;
mod capabilities;
mod cast;
mod components;
// A server and a token of one's own for development (`src/config.rs`, not in git), else none:
// see build.rs.
#[cfg_attr(remux_local_config, path = "config.rs")]
#[cfg_attr(not(remux_local_config), path = "config_none.rs")]
mod config;
mod hooks;
mod log_text;
mod media;
mod music;
mod nav;
mod sdks;
mod server;
mod session;
mod settings;
mod tmdb;
mod tv_focus;
mod user_data;
mod pages;
mod refresh;
mod player;
mod shell;
mod stack;
mod ui;

use nsh_base::theme;
use nsh_core::element::desktop::window as app_window;
use nsh_core::prelude::*;

/// The app's UI set (`nsh.json` `"rust"`).
#[native_module("remux/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
    registry.assets(assets!());
}

/// The app's own native components on Android: the video player (ExoPlayer).
#[cfg(target_os = "android")]
#[native_components(android)]
fn components(components: &nsh::Components) {
    player::exo::register(components);
    music::android::register(components);
    ui::spinner_android::register(components);
}

/// The app's root: signs in, then shows the shell.
#[component]
pub fn App() -> impl IntoElement {
    // The account used last, if its token is still good; else the login screen.
    let restoring = resource(|| Some(()), |(), _| async { hooks::sign_in().await.map_err(|error| error.to_string()) });
    let content = move || {
        view()
            .flex_1()
            .bg(theme().background)
            .text_color(theme().foreground)
            .child(dynamic(move || {
                if hooks::session().is_some() {
                    untracked(|| shell::shell().into_element())
                } else if restoring.get().is_some() || restoring.error().is_some() {
                    untracked(|| pages::login::login().into_element())
                } else {
                    view().flex_1().items_center().justify_center().child(ui::spinner()).into_element()
                }
            }))
    };
    app_window().id("main").content(content)
}
