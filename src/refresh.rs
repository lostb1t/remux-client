//! Pull to refresh on a phone (and in a phone's browser): a page's scroll view pulled down past its
//! top asks the server again. What the app keeps of the server's answers is out of date then
//! (`server::invalidate_cache`), and the pages that read [`pulls`] make what they show again. A TV
//! has no touch: its scroll views are left as they are.

use nsh_core::prelude::*;

use crate::ui::tokens::is_tv;

/// How often a page was pulled down, and whether the spinner of the pull shows.
#[derive(Clone, Copy, Default, PartialEq)]
struct Pulls(u32);

#[derive(Clone, Copy, Default, PartialEq)]
struct Busy(bool);

/// How often a page was pulled down (tracked where it is read): what reads it is made again.
pub fn pulls() -> u32 {
    global::<Pulls>().get().0
}

/// `scroll` with pull to refresh.
pub fn refreshable<S: Scrollable>(scroll: S) -> S {
    refreshable_with(scroll, || {})
}

/// `scroll` with pull to refresh, which also runs `on_pull` (a list that loads its own pages).
pub fn refreshable_with<S: Scrollable>(scroll: S, on_pull: impl Fn() + 'static) -> S {
    if is_tv() {
        return scroll;
    }
    scroll.refreshing(|| global::<Busy>().get().0).on_refresh(move || {
        on_pull();
        pull();
    })
}

/// What a pull does, for what takes a pull itself (the hero's carousel takes every drag that
/// starts on it, the downward one too): the pages ask again, under the spinner of the page.
pub fn pull() {
    crate::server::invalidate_cache();
    global::<Pulls>().update(|pulls| pulls.0 += 1);
    global::<Busy>().set(Busy(true));
    // Long enough to be seen; what is asked for comes in behind it, with its own placeholders.
    set_timeout(std::time::Duration::from_millis(900), || global::<Busy>().set(Busy(false)));
}
