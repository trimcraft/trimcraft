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
- Actions are keyboard macros (key down, key up, tap with hold time, wait), mouse macros
  (button down, up, click, wheel, relative move, move to a position, recorded paths, wait),
  Soundpad commands and Discord mute/deafen. Keys are sent as scan codes so games that read
  DirectInput or RawInput see them. Every executed action is logged.
- **Soundpad** actions play a sound (on speakers, on the mic, or both), stop, pause/resume, or
  play a random, the next or the previous sound. Pick the sound from Soundpad's live list; it is
  stored by title and file path, so rearranging the list in Soundpad does not break it. Soundpad
  (full version, with its remote control) must be running, and the Soundpad connection must be
  enabled under **Connections**.
- **Discord** actions mute, unmute, deafen, undeafen, or toggle mute or deafen. Mute and deafen
  set the state (nothing is sent when it already holds); the toggles go by Discord's live state,
  so muting by hand in Discord is taken into account. Discord must be running and the Discord
  connection enabled under **Connections**.
- At the ends a radio wraps around or stops; selecting the active state again can turn it off;
  at startup the last state is restored (without running actions) or none is active.
- On the overlay a radio shows its name and all its states, vertically or horizontally, with the
  active state highlighted.

### Recording macros

Every keyboard and mouse macro has a **● Record** button. After a 3-second countdown (switch
to the game now) Detent records into that macro until you press the stop hotkey (default
`Ctrl+Shift+F9`, configurable under *Macro recorder*) or **Stop**; **Esc** cancels. While
recording, the main window shows a red banner and the overlay a red **REC** badge. The recording
is appended to the steps or replaces them, as chosen next to the button. The stop hotkey's keys
and the click on Stop are not recorded; Detent's own playback never is (injected input is
ignored). Esc cannot be recorded; add it by hand if a macro needs it.

- A keyboard macro records keys (as scan codes, like playback); a mouse macro records buttons,
  wheel and movement. Gaps become waits, and press-wait-release becomes a tap or click.
- Mouse movement is **absolute** (cursor positions, stored relative to the monitor the
  recording started on, like overlay positions; for menus and maps) or **relative** (raw mouse
  deltas via Raw Input; for mouse-look, where the game keeps the cursor centred). Set it per
  macro before recording.
- Movement is stored as paths of waypoints: thinned to the sample interval and simplified
  (Ramer–Douglas–Peucker on position and time, so pauses and changes of speed survive) within
  the tolerance. Playback interpolates between waypoints at 125 Hz.
- Playback options per macro: speed (original, 0.25× to 4×; scales waits and paths, not hold
  times), skipping waits shorter than a threshold, and for absolute paths "endpoint only",
  which jumps straight to where each path ends.
- The editor has a step list (select several to delete, drag ⋮⋮ to reorder, + inserts a step,
  everything editable in place) and, for mouse macros, a preview of the movement on the
  monitor outline where waypoints can be dragged. **▶ Test play** plays the macro as edited
  after the same countdown.

Relative moves are sent as relative mouse input: games that read raw input see exactly the
recorded deltas, while the Windows desktop applies its pointer speed and acceleration to them.

Windows does not deliver synthesised input to programs running with higher privileges: if a
game runs as administrator, Detent must too for its macros to reach the game (and for the
recorder to see input while the game has focus).

Mapping to a virtual joystick and per-game profiles come in later stages.

## Connections

The **Connections** section lists the programs Detent can control. Soundpad is the first. Each
connection can be enabled, has its own settings (for Soundpad: timeout per request, how old the
cached sound list may get), a live status (disabled, connecting, connected, not reachable) and a
**Test connection** button. While enabled, Detent checks the connection every few seconds and
reconnects with a growing pause when the program is gone; the number of sounds is shown.

A connection's actions are offered in the action editor only while it is enabled. Actions of a
connection that is later disabled stay saved, are shown greyed with a link to Connections, and
are skipped (with a log entry) when they fire. Keyboard and mouse macros need no connection.
Connection settings are stored in `setup.json`; secrets are kept in the Windows Credential
Manager (generic credentials `detent/discord/…`), never in the file or the log.

### Discord

The Discord card under **Connections** has a step-by-step setup guide (with a link to the
Developer Portal); when something fails, the error points at the step that fixes it. In short:
Detent talks to the Discord client through its local RPC interface (named pipe
`\\?\pipe\discord-ipc-0`), using your own application from the Discord Developer Portal:

1. In the [Developer Portal](https://discord.com/developers/applications), create an
   application. Under **OAuth2**, copy the **Client ID**, reset and copy the **Client Secret**,
   and add the redirect `https://localhost/` under **Redirects**. Leave **Public Client** off.
2. Unapproved applications may only use RPC for their owner and testers. If Discord refuses
   the request, add your own Discord account under **App Testers** and accept the invitation.
3. In Detent, under **Connections → Discord**, enter Client ID and Client Secret, **Save**,
   enable the connection and press **Test connection**. Discord shows a consent request for
   `rpc`, `rpc.voice.read` and `rpc.voice.write`; approve it.

The tokens Discord issues are stored in the Credential Manager and refreshed on their own;
later starts need no approval. **Disconnect / forget authorization** deletes them, **Remove
credentials** also deletes the secret. Discord lets only one app at a time change voice
settings and reverts that app's changes when it disconnects, so Detent stays connected while
the connection is enabled (and reconnects when Discord restarts).

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
labels), `setup.json` (encoders, counters, virtual controls, overlay, recorder, connections), `values.json`
(current counter values), `control-states.json` (active radio states).

## CI

Every push to `main` runs the gate and builds the app on Windows and macOS. The Windows build
is downloadable as the `detent-windows` artifact (NSIS installer and plain `detent.exe`).
Builds are not code-signed; Windows SmartScreen will warn on first start.
