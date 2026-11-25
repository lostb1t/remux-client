//! What the viewer did to an item (watched part of it, all of it, marked it, favorited it),
//! told to what is on screen, so it shows without the page being opened again.
//!
//! One change at a time, in a global signal: what shows an item reads [`last`] in a binding or
//! effect, and decides whether the change touches it.

use nsh_core::prelude::*;
use uuid::Uuid;

/// The last change: which items it touched (the item, and the season and the show it is in),
/// and a number that goes up with each one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Change {
    pub seq: u32,
    pub ids: Vec<Uuid>,
}

fn change() -> Signal<Change> {
    global::<Change>()
}

/// Says that the user data of these items changed on the server. Cached lists are out of date
/// from now on, and what shows one of them loads it again.
pub fn changed(ids: impl IntoIterator<Item = Option<Uuid>>) {
    crate::server::invalidate_cache();
    let ids: Vec<Uuid> = ids.into_iter().flatten().collect();
    change().update(|change| {
        change.seq = change.seq.wrapping_add(1);
        change.ids = ids;
    });
}

/// The number of the last change, without listening to it: where an effect starts from.
pub fn seq_now() -> u32 {
    change().with_untracked(|change| change.seq)
}

/// The last change (tracked where it is read): an effect that reads it runs again after each one.
pub fn last() -> Change {
    change().get()
}
