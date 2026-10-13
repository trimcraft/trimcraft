# CLAUDE.md

Guidance for agents working in this repository.

## Project

Trimcraft is a HOTAS configuration tool with an in-game overlay: a public open source project
under GPL-3.0-or-later, maintained by Samuel Breu (@bresam). It currently runs on Windows 10/11;
Linux and macOS support are planned, so keep platform-specific code behind clear module
boundaries. Development and testing happen on Windows. The browser demo (`npm run dev`) and the
mock input backend stay as platform-neutral dev tools.

## Rules

- **Language:** code, comments, commit messages and docs in English.
- **Generic, no vendor special cases.** Never add logic keyed on a specific controller vendor
  or model. The only vendor/product id the code knows is vJoy's (`1234:BEAD`), because Trimcraft
  itself writes to vJoy later. A device whose vendor software sends keystrokes instead of joystick
  input is a normal case for the model, not a special path.
- **Input is event-driven.** Never sample button state per frame. Every edge goes through
  `Engine::apply` as it arrives; the UI may redraw at a lower rate. Encoders depend on this.
- **Logic lives in `trimcraft-core`**, which has no OS dependencies and is fully unit-tested. The
  Tauri crate only wires things together.
- **Thresholds are measured, not assumed.** Encoder detection defaults are deliberately generous
  and the UI shows measured pulse lengths. Don't tune defaults without measurements.
- **Never inject into game processes.** Trimcraft must stay compatible with kernel anti-cheat:
  no graphics hooks, no DLL injection, no reading or writing game memory. The overlay is a
  separate transparent window.
- **Git over HTTPS only** (`origin` is `https://github.com/trimcraft/trimcraft`). No SSH.
- Commits directly on `main` are fine until the first tagged version; after that, feature
  branches from `main` and pull requests.
- "Detent" was an internal working title and is never mentioned publicly (README, docs,
  CHANGELOG, UI). The old name (`detent`, `cloud.breu.detent`, `detent-export`) appears only in
  the migration code and its tests (`migration.rs` in both crates, the old-autostart cleanup in
  `autostart.rs`/`lib.rs`, the schema-5 credential rename in `setup.rs`), the legacy export
  format in `transfer`, and this file. `git grep -i detent` should find nothing else.

## Commands

```sh
./scripts/check.sh                 # full gate (npm ci, svelte-check, build, fmt, clippy -D warnings, tests); run before every push
TRIMCRAFT_INPUT=mock npx tauri dev # run the app with synthetic devices
npm run dev                        # UI in a browser with the in-page demo (ui/src/lib/demo.ts)
cargo run -p trimcraft-input --example probe -- 30
./scripts/third-party-licenses.sh  # regenerate THIRD_PARTY_LICENSES after dependency changes (needs cargo-about)
node scripts/screenshots.mjs docs/images http://localhost:1420 virtual-controls connections hardware settings credits overlay
```

The update check can be tested against a local `latest.json` (a higher `version` and a
`windows-x86_64` entry): debug builds honour `TRIMCRAFT_UPDATE_ENDPOINT=http://127.0.0.1:…/latest.json`
(and `TRIMCRAFT_UPDATE_RECHECK_SECS` for the repeat interval); for a release build add
`--features update-endpoint-override --config '{"plugins":{"updater":{"dangerousInsecureTransportProtocol":true}}}'`
to `npx tauri build`. Never in the workflows (a test checks that).

Docs: `README.md` is the public front page (short); behaviour in detail goes to
`docs/user-guide.md`. Update the guide when you change what the user sees.

CI (`.github/workflows/ci.yml`) runs the gate and the release build as two parallel jobs; build
caches are saved only from `main`, and the release workflow reuses the build job's cache except
for builds signed through SignPath, which are always built from source.

On Windows, a checkout in a deep path (e.g. a worktree under `.claude/worktrees`) makes the SDL3
CMake build fail on the 260-character path limit. Point `CARGO_TARGET_DIR` at a short directory.

## Layout

- `crates/trimcraft-core`: `device` (ids, fingerprints, vJoy detection), `registry` (stable identity
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
  and retention), `migration` (taking over a Detent installation: folder, credential and
  autostart plans, secret reference renaming),
  `recording` (recorder events to macro steps, key/mouse timeline merge, stop trimming, hotkey
  parsing), `engine`
  (ties it together, UI commands, queues action requests).
- `crates/trimcraft-input`: `InputBackend` trait, `sdl` (SDL3 via `sdl3-sys`, built from source,
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
  `tray.rs` (tray icon and menu), `updates.rs` (updater: check, install; Settings, Updates), `autostart.rs` (Run key or elevated Task Scheduler task),
  `elevation.rs` (token elevation, UAC relaunch, single instance across elevation levels,
  elevated-foreground warning), `foreground.rs` (the one `SetWinEventHook` foreground watcher,
  feeding profile switching and the elevation warning), `profile_watch.rs` (profile switching
  from the foreground program, process window list),
  `commands.rs` (IPC; payload shapes are tested),
  `backups.rs` (automatic setup backups, hooked into the setup save), `transfer.rs` (export/import/restore
  commands and their dialogs), `migration.rs` (Detent takeover on the first start: folders,
  Credential Manager entries, autostart),
  `storage.rs` (atomic JSON files). The event log is a `Diagnostics` handle shared by the engine, executor and log bridge (`lib.rs`);
  a thread sends new entries to the UI every 100 ms (`trimcraft://diagnostics`).
- `ui/src`: `App.svelte` (main window: Virtual Controls, Connections, Hardware Test; Settings
  via the gear, one component per section in `components/settings`), `Overlay.svelte`
  (overlay window), `components/DiagnosticsView.svelte` + `lib/diagnostics.svelte.ts` (event log tab),
  `ProfilesPanel`/`ProfileSelector` (profiles), `ToastView`, `lib/overlay-style.ts` (themes as CSS variables), `lib/widget-visibility.svelte.ts` (only-after-change widgets), `ImportFlow` + `settings/BackupSettings` (export, import, backups), `lib/types.ts` mirrors the Rust JSON, `lib/backend.ts` switches between
  Tauri IPC and the browser demo.

When changing a serialised Rust type, update `ui/src/lib/types.ts`, the demo, and the payload
test in `src-tauri/src/commands.rs`. When changing `Setup`, keep old files loading (serde defaults)
or bump `SETUP_SCHEMA_VERSION`.

## Releasing

Versions live in four files and must agree (a test and `scripts/check-versions.mjs` check it):
`Cargo.toml` (`[workspace.package]`), `package.json`, `package-lock.json` and
`src-tauri/tauri.conf.json`.

1. Bump the version in all four (`npm version X.Y.Z --no-git-tag-version` updates both npm files).
2. In CHANGELOG.md, move the "Unreleased" entries into `## [X.Y.Z] - YYYY-MM-DD` and add the
   compare links at the bottom. The release notes are this section; the workflow fails without it.
3. Run `./scripts/check.sh`, commit, merge to `main`.
4. Tag that commit `vX.Y.Z` and push the tag: `git tag vX.Y.Z && git push origin vX.Y.Z`.
5. `.github/workflows/release.yml` checks that the tag matches the version, checks that the CI
   workflow passed for the tagged commit (waiting while it still runs; it does not run the gate
   again), builds the NSIS installer with signed updater artifacts (secrets `TAURI_SIGNING_PRIVATE_KEY` and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; `src-tauri/tauri.release.conf.json` turns them on) and
   publishes "Trimcraft vX.Y.Z (early access)" as a normal release with the installer, its `.sig`
   and `latest.json`. Installed copies find it through
   `https://github.com/trimcraft/trimcraft/releases/latest/download/latest.json` (public key in
   `tauri.conf.json`, `plugins.updater`).

Windows code signing (SignPath Foundation) is prepared and switched on by the repository variable
`SIGNPATH_ENABLED=true` (setup: `docs/signpath-setup.md`, configuration: `.signpath/`). Then the
workflow builds `trimcraft.exe`, has SignPath sign it, bundles the installer around it with
`tauri bundle --no-binary-patching`, has SignPath sign the installer, and only then creates the
updater `.sig` and `latest.json` (`scripts/updater-manifest.mjs`) and publishes. Each SignPath
request waits for a manual approval. Only tag runs are submitted; the public "Code signing
policy" section in the README must stay true (what is signed, roles, network use).

Running the workflow by hand (Actions, Release, "Run workflow") is a dry run: it builds and signs
(updater signature only) and keeps the files as an artifact, but publishes nothing. It also needs
a passed CI run for its commit; for a branch, start CI on it by hand first (`gh workflow run
ci.yml --ref <branch>`). Never mark a release as prerelease or
draft: `/releases/latest`, and with it the updater, only sees normal releases.

## Testing with real hardware

Some things need real hardware and an interactive desktop: devices through DirectInput/RawInput,
the overlay over a borderless game, the global hotkey while the game has focus, encoder pulse
timing, the macro recorder (low-level hooks, Raw Input), Soundpad, Discord and Voicemeeter, the
tray, autostart and elevation (UAC, Task Scheduler), speech, sound and volume. Ignored tests need
such a setup:
`cargo test -p trimcraft -- --ignored` (installs the recorder hooks; talks to a running Soundpad
without playing anything; reads Discord voice settings with saved, authorized credentials and
`TRIMCRAFT_DISCORD_CLIENT_ID` set; writes and removes the autostart Run value, so only without
Trimcraft autostart enabled; lists voices, sound outputs and audio programs without playing anything;
asks a running Voicemeeter for its edition).
Use the CI artifact `trimcraft-windows`. Log file: `%LOCALAPPDATA%\cloud.breu.trimcraft\logs\trimcraft.log`.
