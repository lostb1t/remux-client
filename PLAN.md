# Porting Remux to NSH

Remux (`remux-web`, Dioxus in a WebView) as a Native Shell app in Rust. The proof of concept of the
home screen showed the reason: native scrolling and focus, no WebView on weak phones and TVs.

The order: the foundations the rest stands on, then home finished, then the other screens, the
players last. Each step ends with it running on the phone and the TV, and its tests.

## 0. Foundations

- [x] **Copy the data layer over.** `remux-web` is deprecated once this is done, so its code is
  copied into this app, not shared: `media.rs`, `sdks.rs` (its `Api` sends with NSH's `fetch`
  instead of `reqwest`), `server/`, `session.rs` (NSH's storage), `user_data.rs` and `settings.rs`
  (NSH's global signals), `tmdb.rs`; `hooks.rs` holds the session. Left out until they are used:
  `queue_track` (the music player).
- [ ] **Copy the rest with what uses it** (step 3): `capabilities.rs`, `bandwidth.rs`, `platform.rs`,
  `cast/`, `player/`, `music/`.
- [x] **Navigation.** `nav.rs`: the four places, each with a `nav_stack` of pages over it, kept
  while another place shows; the routes of `remux-web/src/main.rs`; the system's back. Pages that
  are not ported yet are placeholders (`pages/placeholder.rs`).
- [x] **Back from a place's first page** goes to Home (a TV's remote, Android's back), as Remux.
- [x] **Session.** Stored accounts (`session.rs` on NSH's storage); `config.rs` is only the first
  sign-in until the login screen is ported.
- [x] **The `ui` layer.** Remux's look on NSH's elements: `sheet` (NSH `sheet`), `menu_group`,
  `menu_row`, `switch_row` (NSH `switch`), `field` (NSH `input`), `chips` (NSH `tabs`), `spinner`,
  `empty`, besides the buttons, cards, skeletons and glass; all shown on the gallery page (from the
  avatar, until Settings is ported).
- [ ] **More `ui` as screens need it.** Remux has no toast, select or popover menu: it uses sheets.
  Add them only when a screen does.
- [x] **TV focus.** `tv_focus.rs` (Remux's `TvFocus`): the app starts on Play; down from the hero's
  buttons or dots to the first row, up and down to the first card of a row, up from the first row
  to Play, up to the menu and down from it to Play; a row with the focus to the middle of the screen
  (`reveal`). Left and right are Android's own. The hero on a TV is one item at a time, changed by
  the dots (a carousel is a stop of its own, where left and right change the item). The app is in
  the TV launcher (`android/`).
- [ ] **TV focus, the rest:** back to the card a page was opened from; a row's first card when it
  was scrolled sideways; the menu panel.
- [ ] **Tests and CI.** `cargo test` with `Headless` per screen; the Maestro flows of `remux-web/e2e`
  on the TV emulator (they find things by their accessibility labels); a nightly build for Android
  (phone and TV), iOS, desktop and web.
- [ ] **Measure.** The swipe benchmark (SurfaceFlinger frame times) on the Galaxy A14, the Mi TV
  and the throttled emulator, for every screen that is ported, against `remux-web`.

## 1. Home, finished

- [ ] Filters: the Movies / Shows / Music chips, genres, hide watched (`HomeMenu`, `HomeFilters`).
- [ ] Hero: going round (NSH's `looping` shows no slides on Android now: report it, or work around
  it), the crossfade of Remux, textless posters from TMDB on a phone, the watchlist button on a Remux
  server, the More sheet (favorite, watched, playlist).
- [ ] Cards: long press for the card menu (`card_menu.rs`), the watchlist badge, the title on a
  card without a picture, captions (name and year).
- [ ] Rows: more cards as a row is scrolled, rows loaded as they come near, music rows (albums,
  artists), what changed elsewhere (watched, favorite) showing in place.
- [ ] Pull to refresh; error and empty states.
- [ ] Wide screens and desktop: the bar across the top, hover states.
- [ ] TV: the menu panel (places, accounts, settings), focus as in step 0.

## Progress (2026-11-25)

- Home: filters (Films / Shows / Music, with music rows), the card menu on a long press and the hero's
  More, more cards as a row is scrolled, names under music cards, the TV menu panel (places and
  Settings). Left: the hero crossfade on a TV.
- Ported: the title's page (hero, info, links, seasons and episodes, cast), Media (libraries,
  a library, a collection, favorites, playlists), a person, Discover and search, the album, artist
  and playlist pages, Login (with Quick Connect and the accounts of the device), Settings (user, Quick
  Connect, password, library order, playback, accounts, log out), Live TV (on now and by kind,
  channels, guide).
- The video player: ExoPlayer (media3 1.11.1, as Remux) as the app's native component `remux.exo`
  (`android/src/.../ExoView.java`, `src/player/exo.rs`), with Remux's stream choice, resume, progress
  reports, next episode, skip intro, subtitles (drawn by the app) and the remote's keys. The
  libraries come from `android/fetch-libs.py` (no Gradle).
- The video player has the audio track choice (a track of the file switched in the player, else
  a new stream from the server from where it was).
- Music: the queue in its own ExoPlayer (`android/src/.../MusicPlayer.java`, the component
  `remux.music` in `src/music/android.rs`), shuffle, repeat, play next and add to queue (the card
  menu), the bar above the tabs, the page of what plays with its queue, Play and Shuffle on an
  album, an artist and a playlist, start / progress / stop to the server.
- Back with a swipe from the left edge on every page opened over a place (`swipe_back`); the
  hero's picture opens the title.
- Music with the screen off: `MusicService.java`, a foreground service with Android's own
  MediaSession and media notification (no media3-session), with the cover; the lock screen, the
  notification's buttons and a headset's keys work, and the app's queue follows.
- HDR: the screen and the HEVC decoder are asked once (`DeviceInfo.java`, through the music
  component, which has the context); the device profile lets HDR through where the screen shows
  it, and Settings > Playback has the HDR switch and what the screen says.
- Settings: Send log to server (the app's own lines of logcat, read on a thread); on a TV the
  sheet takes the remote's focus as it opens, scrolls with it, and the focused row is white.
- The pages leave room for the music bar while something plays (`tokens::bottom_room`).
- Casting from a phone (new on Android: Remux cast from the desktop and iOS only): Remux's
  `cast` (mDNS, the Cast protocol over TLS) in `src/cast`, with Android's multicast lock
  (`DeviceInfo.multicastLock`); the player's cast button and "Play on" sheet, the stream resolved
  for the Chromecast with its own session, the same controls driving it, and the place it was at
  kept when it moves. Not tried on a Chromecast yet.
- A phone's pages over a place are the app's own stack (`src/stack.rs`): the page below stays
  shown, so the swipe from the left edge drags the page away over it, and Back and the back
  arrows slide it away; a TV keeps NSH's `nav_stack` (focus back where a page was opened from).
- Play asks for the version of a film or episode (Remux's sheet, also with one version), and has
  its cast part on its right where there is a Chromecast; the "Cast to" sheet is shared with the
  player (`components/play_choice.rs`).
- The TV launcher has Remux's banner, and the app Remux's icon (`android/res`, `icons/`).
- Left for the players: iOS (`RemuxMpv`) and the desktops; the music bar on a TV (it shows, but
  the remote cannot reach it yet).
- Left elsewhere: the hero crossfade on a TV (Remux's start at boot stays off, as in Remux).

## 2. The other screens

In the order a user meets them:

1. [ ] **Login** (`login.rs`, `accounts.rs`): server address, user and password, Quick Connect,
   several accounts.
2. [ ] **A title's page** (`MediaDetailView`): the hero, information, seasons and episodes, cast,
   similar, play and resume. Opening it from a card (Remux zooms from the poster).
3. [ ] **Libraries** (`MediaView`, `LibraryView`, `FavoritesView`): grids as virtualized lists with
   columns, sort and filter, pages loaded while scrolling.
4. [ ] **Collections and playlists** (`CollectionView`, `PlaylistsView`, `PlaylistView`).
5. [ ] **Discover and search** (`DiscoverHome`, `DiscoverView`, `search.rs`), and a person
   (`PersonView`).
6. [ ] **Settings** (`settings.rs`): playback, the library order, accounts, uploading logs, the
   bandwidth test.
7. [ ] **Live TV** (`LiveTvView`, `LiveChannelsView`, `LiveGuideView`): the guide as a grid.
8. [ ] **Music screens** (`AlbumView`, `ArtistView`, `music.rs`), without playing (step 3).

## 3. The players

The largest part: native players behind the UI, which NSH draws over.

- [ ] **Video, Android**: mpv as a native component (`MpvBridge` of `remux-web/android/MainActivity.kt`
  copied as an Android library the app ships, `nsh.json` `android.libraries`), with the
  controller and reporter copied from `remux-web/src/player`.
- [ ] **Video, iOS**: the `RemuxMpv` framework as a native component.
- [ ] **Video, desktop and web**: mpv on the desktops; on the web the browser's video with Shaka.
- [ ] **Controls**: the overlays, seeking, audio and subtitle tracks, quality, next episode, the
  remote's keys on a TV (`components/player/`).
- [ ] **Music**: ExoPlayer and AVPlayer (`MusicBridge`, `Music.swift`), playing on with the screen
  off, the mini player and the queue.
- [ ] **Casting**: the Cast engine copied from `remux-web/src/cast`, the device picker.

## 4. Release

- [ ] Android TV: a banner for the launcher (the TV launcher entry is there, `android/`), starting
  at boot (Remux's `BootReceiver`), HDR detection.
- [ ] Signing and updates; the nightly to `lostb1t/remux-client`.
- [ ] This app replaces `remux-web`, which is deprecated then: the releases, the e2e flows and
  the docs move here.

## 5. The web version

Remux's web build is what people use without an app: <https://app.remux.media> and the Docker image
`ghcr.io/lostb1t/remux-client` (nginx serving the files; the server's address is asked at sign in).
The NSH port replaces it with `nsh package --target web`: the Rust crate compiled to WebAssembly,
NSH's engine and layout in `nsh.wasm`, the page drawn as DOM by `nsh.js`. Requests go through the
browser's `fetch` (CORS: the Jellyfin/Remux server has to allow the page's origin, as for Remux's
hosted version). Native components stay empty on the web, so every native part needs a web part.

### Progress (2026-11-25)
- Builds and runs: `nsh run --target web` (Homebrew's LLVM as the wasm C compiler,
  `CC_wasm32_unknown_unknown` / `AR_wasm32_unknown_unknown`; the wasm-bindgen CLI the same version
  as the crate, 0.2.130, through `NSH_WASM_BINDGEN`). Casting is native only; the `cached` crate
  is replaced by a small cache of the app's own (its `ahash` turned on getrandom's browser
  backend, which clashes with NSH's own); Send log is not on the web.
- Home, the pages and the version sheet work in desktop and phone browsers, with the server's
  CORS (it allows any origin).
- The video player: a `<video>` of the app's own in the player's view (`player/web.rs`), Shaka
  bundled as a module of the crate (`js/shaka.js`: the page loads no scripts of other origins),
  the same controls. A direct file and an HLS stream played in Chrome. (Headless Chrome's
  hardware decoder on a Mac refused the remuxed Blu-ray stream; with software decoding it plays.)
- The device profile in a browser is the browser's (Remux's `canPlayType` detection), so the
  server converts what it can't play; Safari and every iPhone browser play HLS with Safari's own
  player (Shaka holds the stream in the page, which a phone runs out of memory with); a start
  the browser refuses without a press waits paused with its controls.
- A phone tries the release build (`nsh package --target web`, 13 MB of wasm, 3.3 MB with
  Brotli), not the dev server's (37.6 MB of wasm).
- The hero's picture is a layer of its own under the text (on the web a press on a view around a
  button went to the view).

### 5.1 It builds and starts (the shell, Home, the pages)
- Casting off on the web: `mdns-sd`, `prost`, `rustls` under `cfg(not(target_arch = "wasm32"))`,
  `src/cast` a stub there (Remux's web build had no casting either: the browser's own Cast).
- Threads: what uses `std::thread` (the music and cast readers, Send log's logcat) is Android or
  native only; `uuid` / `getrandom` with the browser's randomness (`wasm_js`), `chrono` with the
  clock of the browser; `cached` without its timers where they need threads.
- Sign in: no token in `config.rs` on the web, the login page (server address, user, Quick
  Connect) as Remux's; the device id and the accounts in NSH's storage (`localStorage`).
- `nsh run --target web` against a server on the local network, which needs CORS for the dev
  server's origin; checked in Chrome, Safari and Firefox.

### 5.2 Desktop and phone browsers
- `platform.idiom` decides: a desktop browser (mouse, wide) gets the wide layout that a TV and a
  tablet have now (`is_wide`), with hover states, the mouse wheel on the rows, keyboard focus
  (Tab, arrows, Escape as Back); a phone browser gets the phone's (tab bar, sheets, the swipe).
- The tab bar on a desktop: Remux's web has its bar at the bottom too (and the music bar on the
  right); kept, unless it should become a side bar.
- Addresses: each page an address of its own (`UrlScheme.setUrl`), so the browser's back and
  forward, reloading and links to a title work (Remux has routes such as `/media/:id`).

### 5.3 The video player
- A `<video>` through `web-sys` (Remux's `player/web.rs`, 455 lines): direct streams as they are,
  HLS natively where the browser can (Safari) and through Shaka elsewhere (`player/shaka.rs`),
  WebVTT subtitles as tracks, a wake lock. To find out first: whether NSH's own `video` element can
  be given Shaka (its DOM node), or the app puts its `<video>` under the page itself.
- The device profile from the browser (`canPlayType` / `MediaSource.isTypeSupported`, Remux's
  `detect()` on wasm), so the server transcodes what the browser does not play.
- The same controls as on the phone and TV, plus the keyboard (space, arrows, F for full screen)
  and full screen through the Fullscreen API.

### 5.4 Music
- Remux's `music/web.rs` (an `<audio>` with the queue in the app, 275 lines) behind the same
  `music::` functions; the Media Session API for the keys of a keyboard and the lock screen.

### 5.5 What a browser cannot do, and the rest
- No HDR detection (the profile says SDR unless the browser says otherwise), no Send log (the
  browser has its console), no casting, no spinner of the system (the ring stays on the web).
- Splash and icon from `icons/icon.png` (`nsh.json` `icon.web`), the page title, `web.base` when
  it is served under a path.

### 5.6 Publish
- A Docker image like Remux's (nginx with the files of `nsh package --target web`), the same name
  so `ghcr.io/lostb1t/remux-client` users get it; app.remux.media switched when it is ready.
- CI: `nsh package --target web` and the image, next to the Android build (step 4).

## Known limits

- **Android 8.0 (API 26) at least.** Fire OS 6 devices (API 25) are left out; Remux supports them
  now.
- **NSH is young** (0.7.0, a release every few days): pin versions, report what breaks
  (`looping`), and keep fixes in the app, not in NSH.
- **No tvOS.** Not a target of NSH.
