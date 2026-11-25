//! Playback: what the server says to play (Remux's `player/`), for the player page.

pub mod api;
pub mod engine;
#[cfg(target_os = "android")]
pub mod exo;
pub mod reporter;
#[cfg(target_arch = "wasm32")]
pub mod shaka;
#[cfg(target_arch = "wasm32")]
pub mod web;
pub mod stream;
