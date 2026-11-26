//! What the server is told of this device ahead of the first playback (Remux's `main.rs` and
//! `bandwidth.rs`): its device profile, which the server keeps on the device to sort the versions
//! of a title by (Direct Play first, the "Transcode" of the version sheet), sent as soon as there
//! is a session; and, with Automatic quality on, how fast the line to the server is, which a Remux
//! server picks versions by (a tenth less, for some room).
//!
//! The speed is measured with the bitrate test that Jellyfin and Remux both have
//! (`/Playback/BitrateTest?size=`): at the sign-in, and when something is played and the last
//! measurement is old (NSH does not tell an app that it came back to the front, as Remux's WebView
//! did). Only on a Remux server: for another one the test would only cost data.

use std::cell::Cell;

use nsh_core::prelude::*;

use crate::capabilities::Capabilities;
use crate::hooks;

/// A first, small pass: about how fast the line is, to size the real one by.
const PROBE_BYTES: u64 = 256 * 1024;
/// The real pass is sized to last about this long, within what may be asked of the line.
const TARGET_SECONDS: u64 = 3;
const MIN_BYTES: u64 = 1_000_000;
const MAX_BYTES: u64 = 25_000_000;
/// A measurement older than this is made again when something is played.
const STALE_MS: f64 = 10.0 * 60.0 * 1000.0;

/// The last speed measured, in bits per second, and when.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Speed(pub Option<(u64, f64)>);

/// Until the server answered, in ms: mostly the line, not the server.
#[derive(Clone, Copy, Default, PartialEq)]
struct Latency(Option<u32>);

/// Whether a test is going on (tracked where it is read).
#[derive(Clone, Copy, Default, PartialEq)]
struct Busy(bool);

thread_local! {
    static RUNNING: Cell<bool> = const { Cell::new(false) };
}

/// The last speed measured (tracked where it is read).
pub fn speed() -> Option<u64> {
    global::<Speed>().get().0.map(|(bits, _)| bits)
}

/// The last latency measured, in ms (tracked where it is read).
pub fn latency() -> Option<u32> {
    global::<Latency>().get().0
}

/// Whether a test is going on (tracked where it is read).
pub fn measuring() -> bool {
    global::<Busy>().get().0
}

/// Measures now, for the person who asks (Settings).
pub fn retest() {
    measure(true);
}

/// Starts it: once, from the app, with the session.
pub fn start() {
    // Measured for each session (another account, another server).
    effect(|| {
        if hooks::session().is_some() {
            untracked(|| measure(true));
        }
    });
    // The profile, again when what it says changes: the session, Automatic quality, the speed.
    effect(|| {
        let Some(server) = hooks::session() else { return };
        let auto = crate::settings::settings().auto_quality;
        let limit = if auto { speed().map(|bits| (bits / 10 * 9) as i64) } else { None };
        spawn(async move {
            // After the screen's HDR is known (the music component asks for it as the app starts).
            timer(std::time::Duration::from_secs(2)).await;
            let mut profile = Capabilities::detect().to_device_profile();
            // Only a Remux server does anything with the speed of the line.
            profile.max_static_bitrate = if server.is_remux().await { limit } else { None };
            if let Err(error) = server.report_device_profile(profile).await {
                log::debug!("device profile not sent: {error:?}");
            }
        });
    });
}

/// Measures again when something is played and the last measurement is old.
pub fn measure_if_stale() {
    let fresh = global::<Speed>().with_untracked(|speed| speed.0.is_some_and(|(_, at)| nsh_core::time::unix_ms() - at < STALE_MS));
    if !fresh {
        measure(false);
    }
}

fn measure(_force: bool) {
    let Some(server) = hooks::session() else { return };
    if RUNNING.with(Cell::get) {
        return;
    }
    RUNNING.with(|running| running.set(true));
    global::<Busy>().set(Busy(true));
    spawn(async move {
        if server.is_remux().await {
            match line_speed(&server).await {
                Ok((bits, latency)) => {
                    log::info!("bandwidth: {:.1} Mbit/s, {latency} ms", bits as f64 / 1_000_000.0);
                    global::<Latency>().set(Latency(Some(latency)));
                    global::<Speed>().set(Speed(Some((bits, nsh_core::time::unix_ms()))));
                }
                Err(error) => log::debug!("bandwidth not measured: {error}"),
            }
        }
        RUNNING.with(|running| running.set(false));
        global::<Busy>().set(Busy(false));
    });
}

/// One download of `size` bytes: how many came, and how long it took (ms).
async fn download(server: &crate::server::JellyfinServer, size: u64) -> Result<(u64, f64), String> {
    let url = server.url_for(&format!("Playback/BitrateTest?size={size}")).map_err(|error| error.to_string())?;
    let request = nsh_core::platform::HttpRequest::new("GET", url.as_str()).header("X-Emby-Token", server.token());
    let started = nsh_core::time::unix_ms();
    let response = fetch(request).await.map_err(|error| format!("{error:?}"))?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    Ok((response.body.len() as u64, nsh_core::time::unix_ms() - started))
}

/// The speed of the line in bits per second. NSH's `fetch` gives the whole answer at once, so the
/// time until the server answers (from the small first pass) is taken off the second.
/// And the latency: about the time of the small first pass, which is mostly the wait.
async fn line_speed(server: &crate::server::JellyfinServer) -> Result<(u64, u32), String> {
    let rate = |bytes: u64, ms: f64| (bytes as f64 * 8.0 * 1000.0 / ms.max(1.0)) as u64;
    let (bytes, probe_ms) = download(server, PROBE_BYTES).await?;
    let probe = rate(bytes, probe_ms);
    // A short download on a fast line says little, so the second one is as long as it may be.
    let size = (probe / 8 * TARGET_SECONDS).clamp(MIN_BYTES, MAX_BYTES);
    let (bytes, ms) = download(server, size).await?;
    // The probe's time is mostly the wait for the server: take a part of it off.
    Ok((rate(bytes, (ms - probe_ms * 0.5).max(ms * 0.5)), (probe_ms * 0.5).round() as u32))
}
