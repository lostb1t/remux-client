# Remux on NSH: design

A proof of concept: Remux's home screen (`remux-web/src/views/home.rs`, `components/hero.rs`,
`components/media_row.rs`, `components/nav.rs`) as a Native Shell app in Rust, to compare how it
runs on weak devices (a Galaxy A14, an onn 4K, a Mi TV) with the WebView app.

## Targets

| Family | Targets | What differs |
|---|---|---|
| Mobile | Android phones (first), iOS | Glass tab bar at the bottom, touch, the hero a tall picture with centered text |
| TV | Android TV | A glass menu pill top left, the remote's focus (posters grow, a ring), bigger cards, no blur |
| Desktop / web | later | Wide hero (from 1024 points): left aligned, the overview, an info button |

## Visual language

Remux's own: dark only (`"appearance": "dark"`), the page `#171717`, white text, frosted glass for
what floats over the page, posters with 8 pt corners, round buttons. Every color is a token of
`theme.json`; the roles shadcn has none for are named there (`glass`, `scrim`, `shade`,
`rating-star`, `skeleton`, ...) and read through `ui::tokens`.

Icons: Phosphor (`nsh-icon-phosphor`), the regular weight and `Fill` for what is on (favorite,
watched): the nearest set to Remux's Ionicons, rounded outlines with filled variants.

## The `ui` layer

`src/ui/`: `tokens` (colors, motion, `is_tv`, `is_wide`), `button` (`text_button`, `icon_button`:
primary / secondary, regular / large, over `nsh_base::button`), `media_card` (over `button`, with
`progress` for how far it was watched), `skeleton_box`, `glass_surface`. Pages compose these.

Not yet: Sheet, Menu, Toast, Tabs (the tab bar is buttons with `selected`), a gallery page.

## Motion inventory: the home screen

Tokens (`ui::tokens::motion`): `INSTANT` 120 ms, `FAST` 180 ms, `BASE` 240 ms; press 0.97 (icon
buttons 0.95); a TV's focused poster 1.12.

| Change | Motion | Reduced motion |
|---|---|---|
| A button pressed | Scale 0.97 (icon 0.95), `FAST`, ease-out | No scale (native: `active`) |
| A card pressed | Scale 0.97, `FAST` | Same, instant |
| A tab pressed | Wash `accent`, scale 0.97, `INSTANT` | Wash only |
| The remote's focus on a poster (TV) | Grows with a ring and a shadow, `FAST`, ease-out: 1.12 for a poster, less for a wide card, so it stays in the 20 pt gap (no `z_index`: it loses the focus) | Ring only |
| A row gets the focus (TV) | Scrolls to the middle of the screen (`reveal`, animated) | Jumps |
| The remote's focus on a button | White fill, ring, 1.05 on a TV, `FAST` | Ring and fill |
| Favorite / watched toggled | Glyph swap (outline to fill), instant | Same |
| Hero item changes | Phone: the carousel slides with the finger, glides to rest (native). TV: the item of the dot with the focus replaces the last at once (Remux crossfades: to do) | Jumps |
| The dot of the item in view | Widens to a pill, `BASE`, ease-in-out | Instant |
| A picture loads | Fades in, `BASE` | Instant |
| Rows and hero loading | Skeleton pulses | Still |
| Scrolling | The platform's own | Same |

Checked in the running app: see the commit or the PR that changes the screen.
