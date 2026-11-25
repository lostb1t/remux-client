# End-to-end tests

[Maestro](https://maestro.dev) flows that drive the app on an Android TV emulator with the remote's
keys, and check where the focus is.

```sh
e2e/run.sh                 # all flows, on the emulator that is connected
e2e/run.sh media-grid      # one flow
```

What `run.sh` does:

1. Turns `accessibility_enabled` off. Remux's WebView needed it on to show Maestro the page; NSH's
   views are Android's own, which Maestro sees without it.
2. Runs the flows in `e2e/flows`, with the app as it is signed in: a debug build with
   `src/config.rs` signs itself in. (Remux's session file does not apply: the NSH app keeps its
   session in NSH's storage. Writing that from here comes with CI.)

`known-failing/search-focus.yaml` is not in the suite (run.sh runs `flows/`): Maestro's
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
