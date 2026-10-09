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

Mapping to keys or a virtual joystick and per-game profiles come in later stages.

## Layout

| Path | What |
|---|---|
| `crates/detent-core` | Platform-independent model: device identity, live state, encoders, counters, overlay layout. All logic is here and unit-tested. |
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
labels), `setup.json` (encoders, counters, overlay), `values.json` (current counter values).

## CI

Every push to `main` runs the gate and builds the app on Windows and macOS. The Windows build
is downloadable as the `detent-windows` artifact (NSIS installer and plain `detent.exe`).
Builds are not code-signed; Windows SmartScreen will warn on first start.
