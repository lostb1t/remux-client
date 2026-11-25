//! The session the app runs in, read from anywhere (Remux's `hooks.rs`, whose `use_session` and
//! `use_server` read a context; here one global signal of the surface).

use std::sync::Arc;

use anyhow::{anyhow, Result};
use nsh_core::prelude::*;

use crate::sdks;
use crate::server::JellyfinServer;
use crate::session::{self, Stored};

/// The signed in server, if any.
#[derive(Clone, Default, PartialEq)]
struct Session(Option<Arc<JellyfinServer>>);

/// The signed in server, if any (tracked where it is read).
pub fn session() -> Option<Arc<JellyfinServer>> {
    global::<Session>().with(|session| session.0.clone())
}

/// The signed in server. Only below the shell, which is not there without a session.
pub fn server() -> Arc<JellyfinServer> {
    global::<Session>()
        .with_untracked(|session| session.0.clone())
        .expect("server() without a session: is this outside the shell?")
}

/// Starts a session with `server`, and keeps it as the account used last.
pub fn set_session(server: JellyfinServer) {
    session::save(&server.to_stored());
    session::remember(&server.to_stored());
    global::<Session>().set(Session(Some(Arc::new(server))));
}

/// Signs out of the session: the login screen shows; the account stays to sign in again.
pub fn sign_out() {
    if let Some(server) = untracked(session) {
        session::clear(&server.to_stored());
    }
    global::<Session>().set(Session(None));
}

/// Signs in with the account used last, if its token is still good: `false` when there is none
/// (the login screen asks). A developer can still name one in `config.rs`.
pub async fn sign_in() -> Result<bool> {
    let server = match session::restore() {
        Some(stored) => JellyfinServer::resume(stored).await?,
        None => None,
    };
    let server = match server {
        Some(server) => server,
        None if !crate::config::TOKEN.is_empty() => from_config().await?,
        None => return Ok(false),
    };
    set_session(server);
    Ok(true)
}

/// The session of `config.rs`: its token, and the user it is of.
async fn from_config() -> Result<JellyfinServer> {
    let (address, token) = (crate::config::SERVER, crate::config::TOKEN);
    let user_id = match crate::config::USER_ID {
        "" => {
            let host = crate::app_host::app_host();
            let auth = sdks::JellyfinAuth {
                client: "Remux".into(),
                device: host.device_name.clone(),
                device_id: host.device_id.clone(),
                version: host.remux_version.clone(),
                token: Some(token.into()),
            };
            sdks::Api::new(address, auth)?.execute(sdks::GetCurrentUser).await?.id
        }
        id => id.parse()?,
    };
    let stored = Stored {
        server_id: String::new(),
        name: address.into(),
        address: address.into(),
        token: token.into(),
        user_id,
        username: String::new(),
    };
    JellyfinServer::resume(stored).await?.ok_or_else(|| anyhow!("the token of config.rs is not good"))
}
