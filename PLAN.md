# Remux on NSH

Remux (`remux-web`, Dioxus in a WebView) rewritten as a Native Shell app in Rust: native scrolling
and focus, and no WebView on weak phones and TVs. `remux-web` is deprecated; its code was copied
here, not shared. This file is where the port stands (2026-11-25) and what is left.

## Done

**Foundations.** Remux's data layer (`media.rs`, `sdks.rs` over NSH's `fetch`, `server/`,
`session.rs` on NSH's storage, `user_data.rs`, `settings.rs`, `tmdb.rs`, `capabilities.rs`,
`player/`, `cast/`), the four places with their pages (`nav.rs`, `shell.rs`), and the app's `ui`
layer on NSH's elements (`src/ui`, shown on the gallery page under Settings > Components).
`src/config.rs` (not in git, see `config.example.rs`) signs a development build in; without it the
app starts at its login page (`build.rs`).

**Screens.** Home (hero, rows that load more as they scroll, the Films / Shows / Music filters,
music rows), a title's page (hero, info, links, seasons and episodes, cast), Media (libraries, a
library, collections, favorites, playlists), a person, Discover and search, the album, artist and
playlist pages, Live TV (on now, channels, guide), Login (Quick Connect, the accounts of the
device), Settings (account, Quick Connect, password, library order, playback with HDR, accounts,
Send log).

**Navigation.** On a phone the pages over a place are the app's own stack (`stack.rs`): the page
below stays shown, so a swipe from the left edge drags the top page away over it, as Back does. A TV
keeps NSH's `nav_stack`.

**TV.** The remote goes where Remux's does (`tv_focus.rs`): Play first, down to the rows, up to the
menu, a row's first card, the focus back on what opened a page when it closes. The menu pill only on
the first page of a place, at its top; Back there goes to the top, then opens the menu, then leaves
(Home) or goes Home. Sheets take the focus as they open (the card menu on Play). The launcher has
Remux's banner.

**Video player.** ExoPlayer (media3 1.11.1) as the native component `remux.exo`
(`android/src/.../ExoView.java`, `src/player/exo.rs`); on the web a `<video>` with Shaka
(`player/web.rs`, `js/shaka.js`; Safari's own HLS on Apple devices). Remux's stream choice and
device profile (the browser's on the web), the version sheet on Play, resume, progress reports,
next episode, skip intro, subtitles, the audio track, the remote's keys, and HDR where the screen
shows it (`DeviceInfo.java`).

**Music.** The queue in its own ExoPlayer (`MusicPlayer.java`, the component `remux.music`):
shuffle, repeat, play next, add to queue, the bar above the tabs, the page of what plays. With the
screen off: a foreground service with a media session and notification (`MusicService.java`).

**Casting.** From an Android phone to a Chromecast (`src/cast`, Remux's): mDNS with Android's
multicast lock, the cast part of Play and the player's cast button, the device's own session.

**Web.** `nsh package --target web` (needs a clang for WebAssembly, e.g. Homebrew's LLVM, and the
wasm-bindgen CLI of the version in Cargo.toml): Home, the pages and the player in desktop and phone
browsers; the server has to allow the page's origin (CORS).

**Tests and CI.** `e2e/` has Remux's Maestro flows, adapted to this app and its TV rules (signs in
at the login page in CI). `.github/workflows` builds every platform on GitHub's runners and
publishes the nightly builds to this repository; the app id stays `com.github.remux` and the key
Remux's, so a build installs over the old app.

## Left

- **iOS**: a video player (Remux's `RemuxMpv` framework as a native component) and music (AVPlayer);
  until then the iOS build plays nothing.
- **Desktop** (macOS, Windows, Linux): a video player (mpv, as Remux) and music.
- **Web**: music (Remux's `music/web.rs`, an `<audio>` with the Media Session API), an address for
  each page (`UrlScheme`, so back, reload and links work), the player's keyboard and full screen,
  the login page instead of a development token.
- **TV**: the music bar cannot be reached with the remote yet; the hero's crossfade.
- **Casting**: not tried on a Chromecast yet.
- **e2e**: `search-focus` is in `e2e/known-failing` (Maestro's text input keeps the focus in the
  field); a flow per new TV rule as they come.
- **Sign-in after updating** from the old app: it kept the session in a file of its own, which this
  app does not read; people sign in again.
- **Measure**: the swipe benchmark on the Galaxy A14, the Mi TV and a throttled emulator, against
  `remux-web`.

## Known limits

- **Android 8.0 (API 26) at least.** Fire OS 6 devices (API 25) are left out; Remux supported them.
- **NSH is young** (0.7.0): pin versions, report what breaks (the carousel's `looping` showed no
  slides on Android), and keep fixes in the app, not in NSH.
- **No tvOS.** Not a target of NSH.
