//! The session, kept as Jellyfin keeps it: in `jellyfin_credentials`, a list of servers with the
//! token of the user for each, and the id of the device in `_deviceId2`. The dashboard of the
//! server and this app, served from the same address, read and write the same entry: one login
//! is a login in both.
//!
//! A browser keeps it in `localStorage`; the other platforms in a file, with the same content.

use serde_json::{json, Value};
use uuid::Uuid;

const CREDENTIALS_KEY: &str = "jellyfin_credentials";
/// The accounts that were signed in, with their tokens, to sign in again without a password. The
/// list of Jellyfin has one user for each server; this one has them all.
const ACCOUNTS_KEY: &str = "remux_accounts";
const DEVICE_ID_KEY: &str = "_deviceId2";
/// What the dashboard called the device id before it was the Jellyfin one.
const DASHBOARD_DEVICE_ID_KEY: &str = "remux_device_id";

/// A session as it is kept: one server, and the user that is logged in on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    pub server_id: String,
    pub name: String,
    pub address: String,
    pub token: String,
    pub user_id: Uuid,
    /// Not part of what Jellyfin keeps: put there by this app, and fetched when it is not there.
    pub username: String,
}

impl Stored {
    /// Whether both are the same user on the same server.
    pub fn is_account_of(&self, other: &Stored) -> bool {
        same_address(&self.address, &other.address) && self.user_id == other.user_id
    }
}

/// Text under a key: NSH's storage, which every platform keeps (a browser in `localStorage`).
pub(crate) mod store {
    use nsh_core::storage::Storage;

    pub fn get(key: &str) -> Option<String> {
        Storage::get_item(key).filter(|text| !text.is_empty())
    }

    pub fn set(key: &str, value: &str) {
        if let Err(error) = Storage::set_item(key, value) {
            log::error!("could not keep {key}: {error:?}");
        }
    }

    pub fn remove(key: &str) {
        Storage::remove_item(key);
    }
}

pub fn now_ms() -> f64 {
    nsh_core::time::unix_ms() as f64
}

/// What is shared with the server's dashboard and Jellyfin Web, served from the same address: on
/// the web their keys as they are in `localStorage`, which NSH's storage keeps under a prefix of
/// the app's (`nsh:<app id>:`), so the dashboard's session is not in it. Elsewhere NSH's storage.
pub(crate) mod shared {
    #[cfg(target_arch = "wasm32")]
    fn local() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok().flatten()
    }

    pub fn get(key: &str) -> Option<String> {
        #[cfg(target_arch = "wasm32")]
        {
            // What the app kept under its prefix before it was kept here comes along.
            local()
                .and_then(|storage| storage.get_item(key).ok().flatten())
                .filter(|text| !text.is_empty())
                .or_else(|| super::store::get(key))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            super::store::get(key)
        }
    }

    pub fn set(key: &str, value: &str) {
        #[cfg(target_arch = "wasm32")]
        {
            match local().map(|storage| storage.set_item(key, value)) {
                Some(Ok(())) => super::store::remove(key),
                _ => log::error!("could not keep {key}"),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        super::store::set(key, value)
    }

    pub fn remove(key: &str) {
        #[cfg(target_arch = "wasm32")]
        if let Some(storage) = local() {
            storage.remove_item(key).ok();
        }
        super::store::remove(key);
    }

    /// Calls `changed` when another tab changes `key` (the dashboard signs in or out). NSH's
    /// storage reports only the keys under its prefix, so this listens to the page itself.
    pub fn on_change(key: &'static str, changed: impl Fn() + 'static) {
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::{closure::Closure, JsCast};
            let Some(window) = web_sys::window() else { return };
            let listener = Closure::<dyn Fn(web_sys::StorageEvent)>::new(move |event: web_sys::StorageEvent| {
                // No key: the whole of `localStorage` was cleared.
                if event.key().is_none_or(|changed_key| changed_key == key) {
                    changed();
                }
            });
            window.add_event_listener_with_callback("storage", listener.as_ref().unchecked_ref()).ok();
            listener.forget();
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (key, changed);
    }
}

/// The address the page was served from, where there is one: a web build served by the server
/// itself signs in to it, as the dashboard did.
fn origin() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()?.location().origin().ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

fn read_root() -> Value {
    shared::get(CREDENTIALS_KEY)
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({ "Servers": [] }))
}

fn text(entry: &Value, field: &str) -> String {
    entry.get(field).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn same_address(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// The session to carry on with: the server of the address the page comes from if it has one,
/// else the one used last. A session from before this was Jellyfin's is carried over.
pub fn restore() -> Option<Stored> {
    let root = read_root();
    let here = origin();
    let found = root
        .get("Servers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let token = text(entry, "AccessToken");
            let user_id = Uuid::parse_str(&text(entry, "UserId")).ok()?;
            let address = text(entry, "ManualAddress");
            if token.is_empty() || address.is_empty() {
                return None;
            }
            let at_hand = here.as_deref().is_some_and(|here| same_address(here, &address));
            let used = entry.get("DateLastAccessed").and_then(Value::as_f64).unwrap_or(0.0);
            Some((
                (at_hand, used as i64),
                Stored {
                    server_id: text(entry, "Id"),
                    name: text(entry, "Name"),
                    address,
                    token,
                    user_id,
                    username: text(entry, "UserName"),
                },
            ))
        })
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, stored)| stored);
    found
}

/// Keeps the session, next to what else is in the list: other servers, and what other parts put
/// on this one.
pub fn save(session: &Stored) {
    let mut root = read_root();
    if !root.get("Servers").is_some_and(Value::is_array) {
        root["Servers"] = json!([]);
    }
    let servers = root["Servers"].as_array_mut().expect("a list");
    let at = servers.iter().position(|entry| {
        (!session.server_id.is_empty() && text(entry, "Id") == session.server_id)
            || same_address(&text(entry, "ManualAddress"), &session.address)
    });
    let entry = match at {
        Some(at) => &mut servers[at],
        None => {
            servers.push(json!({}));
            servers.last_mut().expect("just added")
        }
    };
    entry["Id"] = json!(session.server_id);
    entry["Name"] = json!(session.name);
    entry["ManualAddress"] = json!(session.address);
    entry["UserId"] = json!(session.user_id.simple().to_string());
    entry["AccessToken"] = json!(session.token);
    entry["UserName"] = json!(session.username);
    entry["DateLastAccessed"] = json!(now_ms());
    shared::set(CREDENTIALS_KEY, &root.to_string());
}

/// Ends the session: the server is taken out of the list.
pub fn clear(session: &Stored) {
    let mut root = read_root();
    if let Some(servers) = root.get_mut("Servers").and_then(Value::as_array_mut) {
        servers.retain(|entry| {
            !((!session.server_id.is_empty() && text(entry, "Id") == session.server_id)
                || same_address(&text(entry, "ManualAddress"), &session.address))
        });
        if servers.is_empty() {
            shared::remove(CREDENTIALS_KEY);
        } else {
            shared::set(CREDENTIALS_KEY, &root.to_string());
        }
    }
}

fn account_json(session: &Stored) -> Value {
    json!({
        "Id": session.server_id,
        "Name": session.name,
        "ManualAddress": session.address,
        "UserId": session.user_id.simple().to_string(),
        "AccessToken": session.token,
        "UserName": session.username,
    })
}

fn same_account(entry: &Value, session: &Stored) -> bool {
    same_address(&text(entry, "ManualAddress"), &session.address)
        && Uuid::parse_str(&text(entry, "UserId")).is_ok_and(|id| id == session.user_id)
}

/// The accounts that were signed in on this device, the one used last first.
pub fn accounts() -> Vec<Stored> {
    store::get(ACCOUNTS_KEY)
        .and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok())
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let token = text(&entry, "AccessToken");
            let address = text(&entry, "ManualAddress");
            let user_id = Uuid::parse_str(&text(&entry, "UserId")).ok()?;
            if token.is_empty() || address.is_empty() {
                return None;
            }
            Some(Stored {
                server_id: text(&entry, "Id"),
                name: text(&entry, "Name"),
                address,
                token,
                user_id,
                username: text(&entry, "UserName"),
            })
        })
        .collect()
}

/// Keeps the account to sign in to again, first in the list. One entry for each user of a server.
pub fn remember(session: &Stored) {
    let mut list: Vec<Value> = store::get(ACCOUNTS_KEY)
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    // A name that is not known yet does not take the place of one that is.
    let mut entry = account_json(session);
    if let Some(old) = list.iter().find(|entry| same_account(entry, session)) {
        if session.username.is_empty() {
            entry["UserName"] = json!(text(old, "UserName"));
        }
    }
    list.retain(|entry| !same_account(entry, session));
    list.insert(0, entry);
    store::set(ACCOUNTS_KEY, &Value::Array(list).to_string());
}

/// Takes the account out of the list: its token is of no use any more, or it is not wanted.
pub fn forget(session: &Stored) {
    let mut list: Vec<Value> = store::get(ACCOUNTS_KEY)
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    list.retain(|entry| !same_account(entry, session));
    if list.is_empty() {
        store::remove(ACCOUNTS_KEY);
    } else {
        store::set(ACCOUNTS_KEY, &Value::Array(list).to_string());
    }
}

/// The id of this device, made once: the same for every part of the app that is served from here.
pub fn device_id() -> String {
    if let Some(id) = shared::get(DEVICE_ID_KEY).or_else(|| shared::get(DASHBOARD_DEVICE_ID_KEY)) {
        shared::set(DEVICE_ID_KEY, &id);
        return id;
    }
    let id = Uuid::new_v4().to_string();
    shared::set(DEVICE_ID_KEY, &id);
    id
}

/// Calls `changed` when another part of the app (another tab of the web build) changes the
/// session.
pub fn on_change(changed: impl Fn() + 'static) {
    let changed = std::rc::Rc::new(changed);
    let elsewhere = changed.clone();
    shared::on_change(CREDENTIALS_KEY, move || elsewhere());
    nsh_core::storage::Storage::on_change(move |change| {
        if change.key().is_none_or(|key| key == CREDENTIALS_KEY) {
            changed();
        }
    });
}
