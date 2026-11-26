# Remux on NSH

Remux (`remux-web`, Dioxus in a WebView) rewritten as a Native Shell app in Rust: native scrolling
and focus, and no WebView on weak phones and TVs. `remux-web` is deprecated; its code was copied
here, not shared. This file is where the port stands (2026-11-26) and what is left.

## Done

**Foundations.** Remux's data layer (`media.rs`, `sdks.rs` over NSH's `fetch`, `server/`,
`session.rs` on NSH's storage, `user_data.rs`, `settings.rs`, `tmdb.rs`, `capabilities.rs`,
`player/`, `cast/`), the four places with their pages (`nav.rs`, `shell.rs`), and the app's `ui`
layer on NSH's elements (`src/ui`).
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
next episode, skip intro, subtitles (text, and pictures drawn over the video: `PictureCues.java`),
the audio track, speed and version, Up next, Live TV's channels before and after, the remote
between the controls, Retry and the Dolby fallback, and what the device decodes as Remux asked it
(`DeviceInfo.java`: HEVC 10-bit, each codec's largest picture, HDR, Dolby Vision with a decoder for
it).

**Music.** The queue in its own ExoPlayer (`MusicPlayer.java`, the component `remux.music`):
shuffle, repeat, play next, add to queue, the bar above the tabs, the page of what plays. With the
screen off: a foreground service with a media session and notification (`MusicService.java`).

**Casting.** From an Android phone to a Chromecast (`src/cast`, Remux's): mDNS with Android's
multicast lock, the cast part of Play and the player's cast button, the device's own session.

**Web.** `nsh package --target web` (needs a clang for WebAssembly, e.g. Homebrew's LLVM, and the
wasm-bindgen CLI of the version in Cargo.toml): Home, the pages and the player in desktop and phone
browsers, with the player's keyboard, volume and full screen, and the login page; the server has to
allow the page's origin (CORS). Each page has an address (`#/discover/title/movie/<id>`,
`address.rs`): the browser's back and forward go through the pages, and a reload or a shared link
opens the page.

**Tests and CI.** `e2e/` has Maestro flows for a TV (`e2e/tv`) and a phone (`e2e/phone`), and
`tests/web.test.tsx` tests the web app in a browser (`nsh test --target web`, a desktop's and a
phone's size); all sign in at the login page against the test server and play The Matrix.
`.github/workflows/e2e.yml` runs them side by side on every push and pull request (the TV's in two
halves), and a job that fails stops the run. The nightly builds and publishes every platform's
build to this repository; the app id stays `com.github.remux` and the key Remux's, so a build
installs over the old app.

## Left

- **iOS**: a video player (Remux's `RemuxMpv` framework as a native component) and music (AVPlayer);
  until then the iOS build plays nothing.
- **Desktop** (macOS, Windows, Linux): a video player (mpv, as Remux) and music.
- **Web**: music (Remux's `music/web.rs`, an `<audio>` with the Media Session API).
- **TV**: the music bar cannot be reached with the remote yet; the hero's crossfade.
- **Player**:
  - text subtitles stand on a dark box; Remux drew them white with a black outline;
  - the seek bar does not show what is buffered;
  - a remote's media keys (play/pause, rewind, fast forward) do nothing in the player (nor did
    they in Remux).
- **Casting**: not tried on a Chromecast yet.
- **Remux server**: a subtitle's DisplayTitle leaves out the file's own title, so a forced track
  ("[SRT] ENG Forced", without the file's forced flag) and the full one are both "English - SRT";
  asked of the server, the app keeps the server's names.
- **e2e**: `search-focus` is in `e2e/tv/known-failing` (Maestro's text input keeps the focus in the
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
