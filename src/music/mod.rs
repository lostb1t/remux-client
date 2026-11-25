//! Music: a queue of tracks that plays on, whatever page is open (Remux's `music`), apart from the
//! video player, which plays one thing and ends with it. The queue itself is in the platform's
//! player (on Android `MusicPlayer.java`, through the native component `remux.music`), so the
//! next track starts without the app. Here is what the app knows of it: the queue, the order and
//! how it plays, kept up to date from what the player says.

#[cfg(target_os = "android")]
pub mod android;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use nsh_core::prelude::*;
use serde_json::{Value as JsonValue, json};
use uuid::Uuid;

use crate::capabilities::Capabilities;
use crate::hooks::server;
use crate::media::Media;
use crate::player::engine::StreamKind;
use crate::player::reporter::Reporter;
use crate::player::{api::Stage, stream};
use crate::sdks::PlayMethod;

/// What happens when the queue is at its end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    /// The queue starts again.
    All,
    /// The track that plays plays again.
    One,
}

impl Repeat {
    /// Off, then all, then one, as the button goes.
    pub fn next(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }

    fn code(self) -> i64 {
        match self {
            Repeat::Off => 0,
            Repeat::All => 1,
            Repeat::One => 2,
        }
    }
}

/// What is queued, and in what order.
#[derive(Clone, Default, PartialEq)]
pub struct Queue {
    /// In the order it plays.
    pub tracks: Vec<Media>,
    /// The place of what plays; `None` when nothing is queued.
    pub index: Option<usize>,
    pub shuffle: bool,
    pub repeat: Repeat,
    /// The queue as it was given, for when shuffling is turned off.
    original: Vec<Media>,
}

impl Queue {
    /// The track that plays.
    pub fn current(&self) -> Option<Media> {
        self.tracks.get(self.index?).cloned()
    }
}

/// How what plays goes.
#[derive(Clone, Default, PartialEq)]
pub struct Clock {
    pub position_ms: f64,
    pub duration_ms: f64,
    pub playing: bool,
    pub buffering: bool,
    pub error: Option<String>,
}

/// Whether the page of what plays is open, and whether it shows the queue.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Expanded {
    pub open: bool,
    pub queue: bool,
}

/// The player's element in the shell, which the commands go to.
#[derive(Clone, Copy, Default)]
struct Host(Option<ElementRef>);

pub fn queue() -> Signal<Queue> {
    global::<Queue>()
}

pub fn clock() -> Signal<Clock> {
    global::<Clock>()
}

pub fn expanded() -> Signal<Expanded> {
    global::<Expanded>()
}

/// What is not shown: the streams the server made for tracks, which queue was sent last (an
/// answer for an older one is dropped), whether the first track is still being asked about, and
/// what tells the server of the track that plays.
#[derive(Default)]
struct Backstage {
    urls: HashMap<Uuid, String>,
    generation: u64,
    resolving: bool,
    reporter: Option<(Uuid, Rc<Reporter>)>,
}

thread_local! {
    static BACKSTAGE: RefCell<Backstage> = RefCell::new(Backstage::default());
}

fn backstage<T>(f: impl FnOnce(&mut Backstage) -> T) -> T {
    BACKSTAGE.with(|backstage| f(&mut backstage.borrow_mut()))
}

/// Whether this platform can play a queue.
pub fn available() -> bool {
    cfg!(target_os = "android")
}

/// Sets the element the commands go to (the shell's `remux.music`).
pub fn attach(host: ElementRef) {
    global::<Host>().set(Host(Some(host)));
}

fn command(name: &str, args: JsonValue) {
    if let Host(Some(host)) = global::<Host>().get_untracked() {
        host.command(name, args);
    }
}

/// A random number below `bound` (from a new UUID, which is random).
fn random_below(bound: usize) -> usize {
    (Uuid::new_v4().as_u128() % bound.max(1) as u128) as usize
}

/// The tracks in a random order, but for the one at `first`, which stays in front.
fn shuffled(tracks: &[Media], first: Option<usize>) -> Vec<Media> {
    let mut rest: Vec<Media> = tracks
        .iter()
        .enumerate()
        .filter(|(place, _)| Some(*place) != first)
        .map(|(_, track)| track.clone())
        .collect();
    for place in (1..rest.len()).rev() {
        rest.swap(place, random_below(place + 1));
    }
    first.and_then(|first| tracks.get(first).cloned()).into_iter().chain(rest).collect()
}

/// A track's cover: its own picture, or its album's.
pub fn cover(track: &Media) -> Option<String> {
    let server = crate::hooks::session()?;
    if track.images.poster.is_some() {
        Some(server.cover_url(track.id, 512))
    } else {
        track.album_id.map(|album| server.cover_url(album, 512))
    }
}

/// The tracks as the player takes them.
fn queue_json(tracks: &[Media]) -> JsonValue {
    let Some(server) = crate::hooks::session() else { return json!([]) };
    backstage(|backstage| {
        JsonValue::Array(
            tracks
                .iter()
                .map(|track| {
                    let url = backstage.urls.get(&track.id).cloned().unwrap_or_else(|| server.audio_url(track.id).to_string());
                    json!({
                        "url": url,
                        "title": track.title,
                        "artist": track.artist.clone().unwrap_or_default(),
                        "album": track.album.clone().unwrap_or_default(),
                        "cover": cover(track).unwrap_or_default(),
                        "durationMs": track.runtime_seconds.unwrap_or(0) as f64 * 1000.0,
                    })
                })
                .collect(),
        )
    })
}

/// Asks the server for a stream of the tracks it was not asked about yet, a few at a time:
/// a file the player does not take gets one the server makes, as for a video. Whether any
/// came in.
async fn resolve(tracks: &[Media]) -> bool {
    let missing: Vec<Media> = backstage(|backstage| tracks.iter().filter(|track| !backstage.urls.contains_key(&track.id)).cloned().collect());
    let server = server();
    let capabilities = Capabilities::detect();
    let mut added = false;
    for chunk in missing.chunks(6) {
        let answers = futures::future::join_all(chunk.iter().map(|track| stream::resolve(&server, track, None, &capabilities, None, None))).await;
        for (track, answer) in chunk.iter().zip(answers) {
            // Only a stream the server makes: what plays as it is keeps its file.
            match answer {
                Ok(stream) if stream.kind == StreamKind::Hls => {
                    backstage(|backstage| backstage.urls.insert(track.id, stream.url));
                    added = true;
                }
                Ok(_) => {
                    backstage(|backstage| backstage.urls.insert(track.id, server.audio_url(track.id).to_string()));
                }
                Err(error) => log::warn!("music: no stream for {}: {error}", track.id),
            }
        }
    }
    added
}

/// Gives the player a queue: a new one to start, or the same one in another order or with other
/// tracks while what plays goes on. The first track is asked about before the queue starts, the
/// others while it plays.
fn send(tracks: Vec<Media>, index: usize, start: bool) {
    let generation = backstage(|backstage| {
        backstage.generation += 1;
        backstage.resolving = start;
        backstage.generation
    });
    spawn(async move {
        if start {
            if let Some(first) = tracks.get(index) {
                resolve(std::slice::from_ref(first)).await;
            }
            if backstage(|backstage| backstage.generation) != generation {
                return;
            }
            command("setQueue", json!({ "tracks": queue_json(&tracks), "index": index, "play": true }));
            backstage(|backstage| backstage.resolving = false);
        } else {
            command("replaceQueue", json!({ "tracks": queue_json(&tracks), "index": index }));
        }
        if !resolve(&tracks).await || backstage(|backstage| backstage.generation) != generation {
            return;
        }
        // Where the queue is by now: the tracks go on while the streams come in.
        let (tracks, at) = queue().with_untracked(|queue| (queue.tracks.clone(), queue.index.unwrap_or(index)));
        command("replaceQueue", json!({ "tracks": queue_json(&tracks), "index": at }));
    });
}

/// Plays `tracks` from the one at `start`.
pub fn play(tracks: Vec<Media>, start: usize) {
    if tracks.is_empty() {
        return;
    }
    if !available() {
        // No queue here: the track plays in the video player.
        if let Some(track) = tracks.get(start) {
            crate::pages::player::play(track.clone());
        }
        return;
    }
    let start = start.min(tracks.len() - 1);
    crate::pages::player::close();
    let shuffle = queue().with_untracked(|queue| queue.shuffle);
    let (order, index) = if shuffle { (shuffled(&tracks, Some(start)), 0) } else { (tracks.clone(), start) };
    backstage(|backstage| backstage.urls.clear());
    queue().update(|queue| {
        queue.original = tracks;
        queue.tracks = order.clone();
        queue.index = Some(index);
    });
    clock().set(Clock { playing: true, ..Clock::default() });
    send(order, index, true);
}

/// Plays `tracks` in a random order.
pub fn shuffle_play(tracks: Vec<Media>) {
    if tracks.is_empty() {
        return;
    }
    queue().update(|queue| queue.shuffle = true);
    let start = random_below(tracks.len());
    play(tracks, start);
}

pub fn toggle_play() {
    let playing = clock().with_untracked(|clock| clock.playing);
    if playing {
        pause();
    } else {
        crate::pages::player::close();
        command("play", json!({}));
        clock().update(|clock| clock.playing = true);
    }
}

pub fn pause() {
    if queue().with_untracked(|queue| queue.index.is_none()) {
        return;
    }
    command("pause", json!({}));
    clock().update(|clock| clock.playing = false);
}

pub fn seek(ms: f64) {
    command("seek", json!({ "ms": ms }));
    clock().update(|clock| clock.position_ms = ms);
}

pub fn skip_to(index: usize) {
    if index >= queue().with_untracked(|queue| queue.tracks.len()) {
        return;
    }
    command("skipTo", json!({ "index": index }));
    queue().update(|queue| queue.index = Some(index));
    clock().update(|clock| {
        clock.position_ms = 0.0;
        clock.playing = true;
    });
}

pub fn next() {
    let (index, count, repeat) = queue().with_untracked(|queue| (queue.index, queue.tracks.len(), queue.repeat));
    let Some(index) = index else { return };
    if index + 1 < count {
        skip_to(index + 1);
    } else if repeat != Repeat::Off && count > 0 {
        skip_to(0);
    }
}

/// Back to the start of the track, or to the one before it when it has just begun.
pub fn previous() {
    let Some(index) = queue().with_untracked(|queue| queue.index) else { return };
    if clock().with_untracked(|clock| clock.position_ms) > 3_000.0 || index == 0 {
        seek(0.0);
    } else {
        skip_to(index - 1);
    }
}

pub fn toggle_shuffle() {
    let (on, current, original) = queue().with_untracked(|queue| (!queue.shuffle, queue.current(), queue.original.clone()));
    queue().update(|queue| queue.shuffle = on);
    let Some(current) = current else { return };
    let at = original.iter().position(|track| track.id == current.id);
    let (order, index) = if on { (shuffled(&original, at), 0) } else { (original, at.unwrap_or(0)) };
    set_order(order, index);
}

pub fn cycle_repeat() {
    let repeat = queue().with_untracked(|queue| queue.repeat.next());
    queue().update(|queue| queue.repeat = repeat);
    command("repeat", json!({ "mode": repeat.code() }));
}

/// Puts a track after the one that plays.
pub fn play_next(track: Media) {
    insert(track, true);
}

/// Puts a track at the end of the queue.
pub fn add_to_queue(track: Media) {
    insert(track, false);
}

fn insert(track: Media, next: bool) {
    if !available() {
        return;
    }
    let (mut tracks, index, mut original, current) =
        queue().with_untracked(|queue| (queue.tracks.clone(), queue.index, queue.original.clone(), queue.current()));
    let Some(index) = index.filter(|_| !tracks.is_empty()) else {
        play(vec![track], 0);
        return;
    };
    if next {
        tracks.insert((index + 1).min(tracks.len()), track.clone());
        let at = current.and_then(|current| original.iter().position(|other| other.id == current.id)).map_or(original.len(), |at| at + 1);
        original.insert(at.min(original.len()), track);
    } else {
        tracks.push(track.clone());
        original.push(track);
    }
    queue().update(|queue| queue.original = original);
    set_order(tracks, index);
}

/// Takes a track out of the queue. The one that plays stays.
pub fn remove(place: usize) {
    let (mut tracks, index, mut original) = queue().with_untracked(|queue| (queue.tracks.clone(), queue.index, queue.original.clone()));
    let Some(index) = index else { return };
    if place == index || place >= tracks.len() {
        return;
    }
    let gone = tracks.remove(place);
    original.retain(|track| track.id != gone.id);
    queue().update(|queue| queue.original = original);
    set_order(tracks, if place < index { index - 1 } else { index });
}

/// The queue in another order or with other tracks, while what plays goes on.
fn set_order(tracks: Vec<Media>, index: usize) {
    queue().update(|queue| {
        queue.tracks = tracks.clone();
        queue.index = Some(index);
    });
    send(tracks, index, false);
}

/// Stops, and empties the queue.
pub fn stop() {
    backstage(|backstage| {
        backstage.generation += 1;
        backstage.resolving = false;
    });
    command("close", json!({}));
    queue().update(|queue| {
        queue.tracks.clear();
        queue.original.clear();
        queue.index = None;
    });
    clock().set(Clock::default());
    expanded().set(Expanded::default());
}

/// Takes in what the player says: the queue goes on by itself, so the track and the time are not
/// what they were when the app last asked.
pub fn on_state(state: &JsonValue) {
    // The player has no queue yet, and says nothing that is true.
    if queue().with_untracked(|queue| queue.tracks.is_empty()) || backstage(|backstage| backstage.resolving) {
        return;
    }
    if !state.get("loaded").and_then(JsonValue::as_bool).unwrap_or(false) {
        return;
    }
    let number = |key: &str| state.get(key).and_then(JsonValue::as_f64).unwrap_or(0.0);
    let flag = |key: &str| state.get(key).and_then(JsonValue::as_bool).unwrap_or(false);
    let index = number("index") as usize;
    if queue().with_untracked(|queue| queue.index != Some(index) && index < queue.tracks.len()) {
        queue().update(|queue| queue.index = Some(index));
    }
    let error = state.get("error").and_then(JsonValue::as_str).filter(|error| !error.is_empty()).map(String::from);
    let next = Clock {
        position_ms: number("position"),
        duration_ms: number("duration"),
        playing: flag("playing") && !flag("ended"),
        buffering: flag("buffering"),
        error,
    };
    if clock().with_untracked(|clock| *clock != next) {
        clock().set(next);
    }
    report_progress();
}

/// Tells the server of the track that plays: as it starts and as it ends (another one plays, or
/// none), and every 10 seconds between.
pub fn on_track(current: Option<Uuid>) {
    let position = clock().with_untracked(|clock| clock.position_ms);
    let previous = backstage(|backstage| {
        if backstage.reporter.as_ref().map(|(id, _)| *id) == current {
            return None;
        }
        Some(backstage.reporter.take())
    });
    let Some(previous) = previous else { return };
    if let Some((_, reporter)) = previous {
        spawn(async move { reporter.report(Stage::Stopped, position, false, None, None).await });
    }
    let (Some(id), Some(server)) = (current, crate::hooks::session()) else { return };
    let reporter = Rc::new(Reporter::new(server, id, id, Some(Uuid::new_v4().simple().to_string()), PlayMethod::DirectPlay));
    backstage(|backstage| backstage.reporter = Some((id, reporter.clone())));
    spawn(async move { reporter.report(Stage::Start, 0.0, false, None, None).await });
}

thread_local! {
    static LAST_REPORT: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
}

fn report_progress() {
    let now = nsh_core::time::unix_ms();
    let due = LAST_REPORT.with(|last| match last.get() {
        Some(at) if now - at < 10_000.0 => false,
        _ => {
            last.set(Some(now));
            true
        }
    });
    if !due {
        return;
    }
    let Some(reporter) = backstage(|backstage| backstage.reporter.as_ref().map(|(_, reporter)| reporter.clone())) else { return };
    let (position, paused) = clock().with_untracked(|clock| (clock.position_ms, !clock.playing));
    spawn(async move { reporter.report(Stage::Progress, position, paused, None, None).await });
}
