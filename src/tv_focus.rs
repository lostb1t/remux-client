//! Where a TV's remote goes between the parts of a page, where Android's own search for the
//! nearest view would go elsewhere (Remux's `TvFocus`): down from the hero to the first row, up and
//! down between rows to the first card of a row, up from the first row to Play, up from the hero
//! to the menu. Within a row and along the hero's buttons, left and right are Android's own.
//!
//! Each page has a [`Scope`] of its own, in its context, so a page opened over another does not
//! send the remote to the one under it. The parts register the views the remote jumps to; the keys
//! are taken from Android with `capture_keys` where a part sends them elsewhere.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use nsh_core::keyboard::KeyCapture;
use nsh_core::keymap::Keystroke;
use nsh_core::prelude::*;
use nsh_core::{RevealOptions, ScrollAlignment};

#[derive(Default)]
struct Targets {
    /// The play button of the hero's item in view.
    play: Option<ElementRef>,
    /// The page's scroll view, which goes back to the top with the focus on the hero.
    page: Option<ElementRef>,
    /// What is above the rows where there is no hero (Discover's chips): up from the first row.
    top: Option<ElementRef>,
    /// The first card of each row, by the row's place on the page. A row without cards (an empty
    /// library) has none, and is passed over.
    rows: BTreeMap<usize, (u64, ElementRef)>,
}

thread_local! {
    static SCOPES: RefCell<HashMap<u64, Targets>> = RefCell::default();
    static MENU: Cell<Option<ElementRef>> = const { Cell::new(None) };
    static NEXT: Cell<u64> = const { Cell::new(1) };
    /// The scopes there are, the last made last: the menu goes down into the top one.
    static STACK: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

fn next_id() -> u64 {
    NEXT.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    })
}

/// The targets of one page.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Scope(u64);

impl Scope {
    /// A scope for the page being made, provided to what it makes; gone with the page.
    pub fn provide() -> Scope {
        let scope = Scope(next_id());
        SCOPES.with(|scopes| scopes.borrow_mut().insert(scope.0, Targets::default()));
        STACK.with(|stack| stack.borrow_mut().push(scope.0));
        cleanup(move || {
            SCOPES.with(|scopes| scopes.borrow_mut().remove(&scope.0));
            STACK.with(|stack| stack.borrow_mut().retain(|id| *id != scope.0));
        });
        provide_context(scope);
        scope
    }

    /// The scope of the page made last that is still there: where the menu goes down into.
    pub fn top() -> Scope {
        Scope(STACK.with(|stack| stack.borrow().last().copied().unwrap_or(0)))
    }

    /// The scope of the page being made (read while building, not in a listener).
    pub fn current() -> Scope {
        use_context::<Scope>().unwrap_or(Scope(0))
    }

    fn with<R>(self, f: impl FnOnce(&mut Targets) -> R) -> Option<R> {
        SCOPES.with(|scopes| scopes.borrow_mut().get_mut(&self.0).map(f))
    }

    /// The hero's play button is `target` (of the item in view).
    pub fn set_play(self, target: ElementRef) {
        self.with(|targets| targets.play = Some(target));
    }

    /// Up from the first row goes to `target`, on a page without a hero.
    pub fn set_top(self, target: ElementRef) {
        self.with(|targets| targets.top = Some(target));
    }

    /// The page scrolls in `target`.
    pub fn set_page(self, target: ElementRef) {
        self.with(|targets| targets.page = Some(target));
    }

    /// The first card of the row at `row` is `target`, until the row goes.
    pub fn set_row(self, row: usize, target: ElementRef) {
        let id = next_id();
        self.with(|targets| targets.rows.insert(row, (id, target)));
        cleanup(move || {
            self.with(|targets| {
                if targets.rows.get(&row).is_some_and(|(kept, _)| *kept == id) {
                    targets.rows.remove(&row);
                }
            });
        });
    }

    /// To the hero's play button, with the page back at its top.
    pub fn to_play(self) -> bool {
        let Some((play, page)) = self.with(|targets| (targets.play, targets.page)) else { return false };
        let moved = focus(play);
        // The page to its top after the focus: Android brings a focused view into view itself,
        // which cut a scroll started before it short (the hero half on screen).
        if let Some(page) = page {
            page.scroll_to_offset(px(0.0), true);
            set_timeout(std::time::Duration::from_millis(300), move || page.scroll_to_offset(px(0.0), true));
        }
        if moved {
            // At the top at once, not when the scroll has come there: the menu over it shows,
            // for up to reach.
            mark_top(crate::nav::place_untracked());
        }
        moved
    }

    /// To the first card of the first row after `row` (`None`: the first row of the page).
    pub fn to_row_after(self, row: Option<usize>) -> bool {
        let next = self
            .with(|targets| match row {
                Some(row) => targets.rows.range(row + 1..).next().map(|(_, (_, target))| *target),
                None => targets.rows.values().next().map(|(_, target)| *target),
            })
            .flatten();
        focus_row(next)
    }

    /// To the first card of the row before `row`, or the hero's play button above the first row.
    pub fn to_row_before(self, row: usize) -> bool {
        let before = self.with(|targets| targets.rows.range(..row).next_back().map(|(_, (_, target))| *target)).flatten();
        focus_row(before) || self.to_play() || focus(self.with(|targets| targets.top).flatten())
    }
}

fn focus(target: Option<ElementRef>) -> bool {
    match target {
        Some(target) => {
            target.focus();
            true
        }
        None => false,
    }
}

/// The focus to the first card of a row, and the row to the middle of the screen, so the row
/// below it shows too (Remux: `scroll-margin-block: 30vh`). Android only scrolls it into view.
fn focus_row(target: Option<ElementRef>) -> bool {
    let Some(target) = target else { return false };
    target.focus();
    target.reveal(RevealOptions::default().vertical(ScrollAlignment::Center).horizontal(ScrollAlignment::Nearest));
    true
}

thread_local! {
    /// What was pressed last to open a page (a card, a tile, a row), and what opened each page
    /// of each place, by depth: Back gives the focus back to it, which NSH's stack does not.
    static PRESSED: Cell<Option<(ElementRef, f64)>> = const { Cell::new(None) };
    static OPENERS: RefCell<HashMap<crate::nav::Place, Vec<Option<ElementRef>>>> = RefCell::default();
}

/// `target` was pressed: if it opens a page, Back comes back to it.
pub fn pressed(target: ElementRef) {
    if crate::ui::tokens::is_tv() {
        PRESSED.with(|pressed| pressed.set(Some((target, nsh_core::time::unix_ms()))));
    }
}

/// A page was opened over `place`, at `depth`: by what was pressed last.
pub fn opened(place: crate::nav::Place, depth: usize) {
    // Only a press that opened it: one of a while ago (a row that marked something watched) did not.
    let now = nsh_core::time::unix_ms();
    let opener = PRESSED.with(Cell::take).filter(|(_, at)| now - at < 1000.0).map(|(target, _)| target);
    OPENERS.with(|openers| {
        let mut openers = openers.borrow_mut();
        let list = openers.entry(place).or_default();
        list.truncate(depth.saturating_sub(1));
        list.push(opener);
    });
}

/// The pages of `place` over `depth` closed: the focus back to what opened the lowest of them,
/// once the page under it is shown again (NSH's slide takes 300 ms).
pub fn closed(place: crate::nav::Place, depth: usize) {
    let opener = OPENERS.with(|openers| {
        let mut openers = openers.borrow_mut();
        let list = openers.entry(place).or_default();
        let opener = list.get(depth).copied().flatten();
        list.truncate(depth);
        opener
    });
    if let (true, Some(opener)) = (crate::ui::tokens::is_tv(), opener) {
        // Not a timer of this listener's: it belongs to the page that is going, and goes with it.
        // The shell's, which stays (`give_back`).
        global::<GiveBack>().set(GiveBack(Some(opener)));
    }
}

/// What gets the focus back once the page under a closed one shows again.
#[derive(Clone, Copy, Default)]
struct GiveBack(Option<ElementRef>);

/// The first element of `place` to get the focus once the place shows (a view that is not shown
/// takes none). Through the shell: a timer of the caller's (a row of the menu, which closes with
/// the press) would go with it.
#[derive(Clone, Copy, Default, PartialEq)]
struct FirstLater(Option<(crate::nav::Place, u64)>);

pub fn to_first_later(place: crate::nav::Place) {
    global::<FirstLater>().set(FirstLater(Some((place, next_id()))));
}

/// Gives the focus back to what opened a page that closed, and to a place's first element: made
/// once, by the shell.
pub fn give_back() {
    effect(|| {
        if let FirstLater(Some((place, _))) = global::<FirstLater>().get() {
            set_timeout(std::time::Duration::from_millis(150), move || to_first(place));
        }
    });
    effect(|| {
        if let GiveBack(Some(opener)) = global::<GiveBack>().get() {
            // Once NSH's slide has shown the page under it (300 ms).
            set_timeout(std::time::Duration::from_millis(400), move || opener.focus());
        }
    });
}

/// The menu is `target`: one for the app.
pub fn set_menu(target: ElementRef) {
    MENU.with(|menu| menu.set(Some(target)));
}

/// To the menu (shown first: a hidden view takes no focus).
pub fn to_menu() -> bool {
    mark_top(crate::nav::place_untracked());
    focus(MENU.with(Cell::get))
}

/// The first page of `place` is at its top.
fn mark_top(place: crate::nav::Place) {
    let scrolled = global::<Scrolled>();
    if scrolled.with_untracked(|scrolled| scrolled.0[place.index()]) {
        scrolled.update(|scrolled| scrolled.0[place.index()] = false);
    }
}

/// The arrow keys a part takes from Android, to send the focus itself.
pub fn capture(directions: &[&str]) -> KeyCapture {
    let keys: Vec<Keystroke> = directions.iter().filter_map(|key| Keystroke::parse(key, os()).ok()).collect();
    KeyCapture::Keys(Arc::from(keys))
}

/// Whether the first page of each place is scrolled away from its top (by place), and the scroll
/// view of each, for Back on a TV: away from the top it goes back up, at the top it opens the
/// menu, which only shows there.
#[derive(Clone, Copy, Default, PartialEq)]
struct Scrolled([bool; 4]);

thread_local! {
    static ROOT_SCROLLS: RefCell<HashMap<crate::nav::Place, ElementRef>> = RefCell::default();
}

/// The first page of `place` scrolls in `target`: what its scroll view calls as it scrolls.
pub fn root_scroll(place: crate::nav::Place, target: ElementRef) -> impl Fn(&ScrollEvent) + Copy + 'static {
    ROOT_SCROLLS.with(|scrolls| scrolls.borrow_mut().insert(place, target));
    move |event: &ScrollEvent| {
        let down = event.offset.y.0 > 24.0;
        let scrolled = global::<Scrolled>();
        if scrolled.with_untracked(|scrolled| scrolled.0[place.index()] != down) {
            scrolled.update(|scrolled| scrolled.0[place.index()] = down);
        }
    }
}

/// Whether the first page of `place` is at its top (tracked where it is read).
pub fn at_top(place: crate::nav::Place) -> bool {
    !global::<Scrolled>().with(|scrolled| scrolled.0[place.index()])
}

/// The first page of `place` back at its top: the focus on the hero's play button where it has
/// one, else on the menu.
pub fn to_top(place: crate::nav::Place) {
    if let Some(page) = ROOT_SCROLLS.with(|scrolls| scrolls.borrow().get(&place).copied()) {
        page.scroll_to_offset(px(0.0), true);
    }
    global::<Scrolled>().update(|scrolled| scrolled.0[place.index()] = false);
    to_first(place);
}

thread_local! {
    /// What the remote starts on in each place's first page: Home's Play, Discover's search
    /// field, Media's first tile, Live TV's Channels.
    static FIRSTS: RefCell<HashMap<crate::nav::Place, Rc<dyn Fn() -> bool>>> = RefCell::default();
}

/// The first page of `place` starts on what `focus` gives the focus to (whether it could).
pub fn set_first(place: crate::nav::Place, focus: impl Fn() -> bool + 'static) {
    FIRSTS.with(|firsts| firsts.borrow_mut().insert(place, Rc::new(focus)));
}

/// The first page of `place` starts on `target`.
pub fn set_first_view(place: crate::nav::Place, target: ElementRef) {
    set_first(place, move || {
        target.focus();
        true
    });
}

/// The focus to the first element of `place`'s first page (else the menu).
pub fn to_first(place: crate::nav::Place) {
    let first = FIRSTS.with(|firsts| firsts.borrow().get(&place).cloned());
    if !first.is_some_and(|first| first()) {
        to_menu();
    }
}
