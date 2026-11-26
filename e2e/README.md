# End-to-end tests

[Maestro](https://maestro.dev) flows that drive the app on Android emulators: a TV with the
remote's keys (`tv/flows`), a phone with a finger (`phone/flows`), sharing `lib/` (signing in,
opening a page, waiting).

```sh
e2e/run.sh                          # the TV's flows, on the emulator that is connected
E2E_KIND=phone e2e/run.sh           # the phone's
e2e/run.sh media-grid               # one flow
E2E_SHARD=1/2 e2e/run.sh            # every other flow, from the first (CI runs halves side by side)
```

The web app has tests of its own, in a browser: `tests/web.test.tsx` (`nsh test`, not Maestro),
with the server and user in `tests/test-server.ts` (not in git):

```sh
echo 'export const server = "https://…", username = "…", password = "…";' > tests/test-server.ts
nsh test tests/web.test.tsx --target web     # headless Chrome; phone size and a desktop's
```

On GitHub (`.github/workflows/e2e.yml`, on every push and pull request, and before a nightly
release) the TV's flows run in two halves, the phone's and the web's beside them, each on a runner
of its own.

Our own buttons that share a name with Android's (the tab bar's Home, a page's Back) have test ids
for the flows (`tab-home`, `tab-media`, `tab-discover`, `tab-live-tv`, `tab-settings`, `page-back`):
Maestro sees the system's navigation bar too.

What `run.sh` does:

1. Turns `accessibility_enabled` off. Remux's WebView needed it on to show Maestro the page; NSH's
   views are Android's own, which Maestro sees without it.
2. Runs the flows in `e2e/flows`, with the app as it is signed in: a debug build with
   `src/config.rs` signs itself in. (Remux's session file does not apply: the NSH app keeps its
   session in NSH's storage. Writing that from here comes with CI.)

`tv/known-failing/search-focus.yaml` is not in the suite (run.sh runs `tv/flows/`): Maestro's
`inputText` sets the field's text through the accessibility API, after which the field keeps the
focus and the keyboard, and neither Back nor the remote's keys from Maestro get it out. Typed with
the remote's keyboard (or `adb shell input text` and Enter), the app moves the focus to the first
result as it should, and up from there is the Genre chip. Run it by hand:
`e2e/run.sh ../known-failing/search-focus`.

The flows are Remux's, with the NSH app's id and its TV rules: the menu shows only on the first
page of a place, at its top (`grid-sideways`: up from a page over a place stays on it), and
`lib/focus-tile.yaml` finds a tile of the Media page wherever the server puts it.

`env.sh` has what Maestro needs: a JDK, adb, and a PATH without `flutter` (Maestro runs it at
start up, and it can hang).

The phone's `play` flow plays The Matrix, which the test server plays.
