# Detent

Configuration tool for HOTAS controllers (stick, throttle, pedals, button boxes) on Windows,
with an in-game overlay.

> A *detent* is the tactile notch on a throttle lever where it clicks into place.

## What it does (stage 1)

- Detects every connected game controller and shows all axes, buttons and hats live.
- Gives each device a **stable identity** across restarts and re-plugging, including two units
  of the same model, and marks virtual devices (vJoy) so they are never mistaken for input.
- Recognises **rotary encoders** (two buttons that pulse once per detent) by their behaviour, or
  lets you mark a button pair by hand, and shows measured pulse lengths.
- **Counters** with range, step and wrap-around or stop, driven by an encoder, e.g. a radio
  channel.
- An **overlay**: a transparent, click-through, always-on-top window showing selected axes,
  buttons, hats and counters. Toggle it with a global hotkey (default `Ctrl+Shift+F10`).

The overlay never touches the game process. It works over games in windowed or borderless
fullscreen mode, not exclusive fullscreen.

The overlay covers one monitor of your choice (primary by default). Every widget can be placed
freely: press **Edit layout**, drag the widgets on the overlay, then **Done** (or Esc). Positions
are stored relative to the monitor, so they survive resolution changes; X/Y can also be typed in.
Widgets that were never placed stay stacked in the chosen corner, as in earlier versions.

## Virtual controls

The main window has two sections: **Hardware Test** (everything above) and **Virtual Controls**.
A virtual control is built from physical inputs and can run actions. The first kind is the
**radio**: several states of which at most one is active, like the preset buttons of a car radio.

- Triggers: a button press, a hat direction being entered, or one encoder detent. Next state,
  previous state, "all off", "toggle off" and "select this state" each take any number of
  triggers. Assign them by pressing the control ("learn") or pick them from a list. Toggle off
  turns the active state off, or brings back the most recently active one (the first state if
  none was active yet); this also works after a restart.
- Each state has actions on activate and on deactivate. Switching from A to B runs A's
  deactivate actions, then B's activate actions.
- Actions are keyboard macros (key down, key up, tap with hold time, wait) and mouse macros
  (button down, up, click, wheel, relative move, wait). Keys are sent as scan codes so games
  that read DirectInput or RawInput see them. Every executed action is logged.
- **Soundpad** actions play a sound (on speakers, on the mic, or both), stop, pause/resume, or
  play a random, the next or the previous sound. Pick the sound from Soundpad's live list; it is
  stored by title and file path, so rearranging the list in Soundpad does not break it. Soundpad
  (full version, with its remote control) must be running; the editor shows whether it is
  reachable, and a missing sound or Soundpad is logged and shown in Virtual Controls.
- At the ends a radio wraps around or stops; selecting the active state again can turn it off;
  at startup the last state is restored (without running actions) or none is active.
- On the overlay a radio shows its name and all its states, vertically or horizontally, with the
  active state highlighted.

Windows does not deliver synthesised input to programs running with higher privileges: if a
game runs as administrator, Detent must too for its macros to reach the game.

Mapping to a virtual joystick and per-game profiles come in later stages.

## Layout

| Path | What |
|---|---|
| `crates/detent-core` | Platform-independent model: device identity, live state, encoders, counters, virtual controls, actions, overlay layout. All logic is here and unit-tested. |
| `crates/detent-input` | Input backends: SDL3 (Windows, macOS) and a mock with synthetic devices. |
| `src-tauri` | Desktop app (Tauri 2): engine thread, windows, hotkey, persistence. |
| `ui` | Svelte 5 + TypeScript frontend: main window and overlay window. |

## Development

Requirements: Rust (stable), Node 24, CMake (SDL3 is built from source and linked statically).

```sh
npm install
npx tauri dev                       # app with real devices
DETENT_INPUT=mock npx tauri dev     # app with synthetic devices (no hardware needed)
npm run dev                         # UI only, in a browser, with an in-page demo
cargo run -p detent-input --example probe -- 30   # print raw input events for 30 s
./scripts/check.sh                  # full gate, same as CI; run before every push
```

Settings live in the app config directory (`%APPDATA%\cloud.breu.detent` on Windows,
`~/Library/Application Support/cloud.breu.detent` on macOS): `devices.json` (known devices and
labels), `setup.json` (encoders, counters, virtual controls, overlay), `values.json` (current counter
values), `control-states.json` (active radio states).

## CI

Every push to `main` runs the gate and builds the app on Windows and macOS. The Windows build
is downloadable as the `detent-windows` artifact (NSIS installer and plain `detent.exe`).
Builds are not code-signed; Windows SmartScreen will warn on first start.
