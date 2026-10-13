# CLAUDE.md

Guidance for agents working in this repository.

## Project

Detent is a HOTAS configuration tool for Windows with an in-game overlay. Owner: Sam
(private project). Detent supports Windows 10/11 only; development and testing happen on
Windows. The browser demo (`npm run dev`) and the mock input backend stay as platform-neutral
dev tools.

## Rules

- **Language:** code, comments, commit messages and docs in English.
- **Generic, no vendor special cases.** Never add logic keyed on a specific controller vendor
  or model. The only vendor/product id the code knows is vJoy's (`1234:BEAD`), because Detent
  itself writes to vJoy later. A device whose vendor software sends keystrokes instead of joystick
  input is a normal case for the model, not a special path.
- **Input is event-driven.** Never sample button state per frame. Every edge goes through
  `Engine::apply` as it arrives; the UI may redraw at a lower rate. Encoders depend on this.
- **Logic lives in `detent-core`**, which has no OS dependencies and is fully unit-tested. The
  Tauri crate only wires things together.
- **Thresholds are measured, not assumed.** Encoder detection defaults are deliberately generous
  and the UI shows measured pulse lengths. Don't tune defaults without measurements.
- **The overlay never injects into the game.** It is a separate transparent window. Do not add
  graphics hooks or DLL injection; games with kernel anti-cheat (e.g. Wardogs) would flag them.
- **Git over HTTPS only** (`origin` is `https://github.com/bresam/detent.git`). No SSH.
- Commits directly on `main` are fine until the first tagged version; after that, feature
  branches from `main` and pull requests.

## Commands

```sh
./scripts/check.sh                 # full gate (npm ci, svelte-check, build, fmt, clippy -D warnings, tests); run before every push
DETENT_INPUT=mock npx tauri dev    # run the app with synthetic devices
npm run dev                        # UI in a browser with the in-page demo (ui/src/lib/demo.ts)
cargo run -p detent-input --example probe -- 30
```

On Windows, a checkout in a deep path (e.g. a worktree under `.claude/worktrees`) makes the SDL3
CMake build fail on the 260-character path limit. Point `CARGO_TARGET_DIR` at a short directory.

## Layout

- `crates/detent-core`: `device` (ids, fingerprints, vJoy detection), `registry` (stable identity
  resolution), `state` (live input), `encoder` (detection, pulse stats), `function` (counters),
  `setup` (persisted config, schema-versioned, migrations), `profile` (per-game profiles,
  executable matching, auto-switch decisions), `settings` (app-wide settings outside the
  profiles, autostart mechanism choice, hidden start, elevation warning rules),
  `trigger` (trigger inputs, press kinds and the deadline-based press tracker, modifiers,
  conditions, axis zones), `virtual_control` (radio, toggle, momentary, counter, mode;
  commands and transitions),
  `connection` (connector settings, status, gating of connector actions; secrets only by
  reference),
  `action` (actions, key table with scan codes, expansion to input ops), `macros` (input macros:
  mixed steps, repetition, retrigger), `macro_run` (macro player decisions: repetition state,
  held-input bookkeeping, registry),
  `audio` (speech, sound file, volume and media key actions), `voicemeeter` (parameters,
  gain math), `launch` (launch targets, URL check), `webhook` (HTTP requests, secret header
  references, URL redaction), `placeholder` (`{control}` and friends in action texts),
  `soundpad` (Soundpad commands, request strings, sound list parsing and lookup), `discord`
  (Discord RPC frames, messages, OAuth login flow over a `Transport` trait, idempotent
  mute/deafen), `path`
  (mouse path simplification, playback schedule, monitor and SendInput coordinates),
  `diagnostics` (event log: ring buffer, axis throttle, entry formatting),
  `toast` (toast queue: newest first, one per control, at most 3, expiry),
  `vjoy` (vJoy targets, axis curves, mappings, desired output state and its diff),
  `transfer` (export files, secret stripping, import validation, profile merge), `backup` (backup file names
  and retention),
  `recording` (recorder events to macro steps, key/mouse timeline merge, stop trimming, hotkey
  parsing), `engine`
  (ties it together, UI commands, queues action requests).
- `crates/detent-input`: `InputBackend` trait, `sdl` (SDL3 via `sdl3-sys`, built from source,
  static), `mock`.
- `src-tauri`: `lib.rs` (engine thread, persistence), `executor.rs` (action thread), `player.rs` (input macro
  player: repetition, stop, release of held inputs, panic), `input.rs` (`SendInput`),
  `soundpad.rs` (Soundpad connector: pipe client, monitor with reconnect, worker thread),
  `voicemeeter.rs` (Voicemeeter connector: VoicemeeterRemote64.dll loaded at runtime),
  `speech.rs` (SAPI thread), `sound.rs` (rodio thread), `volume.rs` (Core Audio),
  `launch.rs` (CreateProcess, ShellExecuteW), `webhook.rs` (ureq, secret header values),
  `discord.rs` (Discord connector: IPC pipe, token endpoint over HTTPS, worker thread with
  reconnect), `vjoy.rs` (vJoy connector: `vJoyInterface.dll` loaded at run time behind the
  `VjoyApi` trait, worker thread writing output diffs), `secrets.rs` (credential store via
  `keyring`),
  `recorder.rs` (macro recorder:
  low-level hooks and Raw Input on a thread of its own), `overlay.rs` (overlay window, hotkey), `toasts.rs` (toast queue thread; shows the window without activating it),
  `tray.rs` (tray icon and menu), `autostart.rs` (Run key or elevated Task Scheduler task),
  `elevation.rs` (token elevation, UAC relaunch, single instance across elevation levels,
  elevated-foreground warning), `foreground.rs` (the one `SetWinEventHook` foreground watcher,
  feeding profile switching and the elevation warning), `profile_watch.rs` (profile switching
  from the foreground program, process window list),
  `commands.rs` (IPC; payload shapes are tested),
  `backups.rs` (automatic setup backups, hooked into the setup save), `transfer.rs` (export/import/restore
  commands and their dialogs),
  `storage.rs` (atomic JSON files). The event log is a `Diagnostics` handle shared by the engine, executor and log bridge (`lib.rs`);
  a thread sends new entries to the UI every 100 ms (`detent://diagnostics`).
- `ui/src`: `App.svelte` (main window: Virtual Controls, Connections, Hardware Test; Settings
  via the gear, one component per section in `components/settings`), `Overlay.svelte`
  (overlay window), `components/DiagnosticsView.svelte` + `lib/diagnostics.svelte.ts` (event log tab),
  `ProfilesPanel`/`ProfileSelector` (profiles), `ToastView`, `lib/overlay-style.ts` (themes as CSS variables), `lib/widget-visibility.svelte.ts` (only-after-change widgets), `ImportFlow` + `settings/BackupSettings` (export, import, backups), `lib/types.ts` mirrors the Rust JSON, `lib/backend.ts` switches between
  Tauri IPC and the browser demo.

When changing a serialised Rust type, update `ui/src/lib/types.ts`, the demo, and the payload
test in `src-tauri/src/commands.rs`. When changing `Setup`, keep old files loading (serde defaults)
or bump `SETUP_SCHEMA_VERSION`.

## Testing with real hardware

Some things need real hardware and an interactive desktop: devices through DirectInput/RawInput,
the overlay over a borderless game, the global hotkey while the game has focus, encoder pulse
timing, the macro recorder (low-level hooks, Raw Input), Soundpad, Discord and Voicemeeter, the
tray, autostart and elevation (UAC, Task Scheduler), speech, sound and volume. Ignored tests need
such a setup:
`cargo test -p detent -- --ignored` (installs the recorder hooks; talks to a running Soundpad
without playing anything; reads Discord voice settings with saved, authorized credentials and
`DETENT_DISCORD_CLIENT_ID` set; writes and removes the autostart Run value, so only without
Detent autostart enabled; lists voices, sound outputs and audio programs without playing anything;
asks a running Voicemeeter for its edition).
Use the CI artifact `detent-windows`. Log file: `%LOCALAPPDATA%\cloud.breu.detent\logs\detent.log`.
