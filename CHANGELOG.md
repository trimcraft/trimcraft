# Changelog

All notable changes to Trimcraft are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Until 1.0, releases are early access:
expect rough edges, and back up your configuration before updating.

## [Unreleased]

## [0.1.0] - 2026-10-13

The first public release (early access). Windows 10/11 only.

### Added

- **Devices and hardware test**: every connected game controller with all axes, buttons and hats
  live; stable identities across restarts and re-plugging, also for two units of the same model;
  vJoy devices are recognised and never used as input.
- **Rotary encoders**: detected by their pulses or marked by hand, with measured pulse lengths;
  counters with range, step and wrap-around driven by an encoder.
- **Virtual controls**: radio, toggle, momentary, counter and mode (layer, with shift states).
  Triggers on buttons, hat directions, encoder detents and axis zones, with press, release, long
  press and double press, modifiers and "only when" conditions. Learn by pressing the control.
- **Actions**: input macros with keyboard and mouse steps in any order (recorded or edited;
  absolute or relative mouse paths, speed, repetition once, n times or while the control is
  active), media keys, speech, sound files, Windows volume per app or master, launching
  programs, URLs and files, HTTP requests (webhooks) with secret headers, profile switches,
  and "Stop macros". Placeholders such as `{state}` and `{value}` in texts.
- **Safety**: stopping a macro releases every key and button it holds; a panic hotkey
  (default `Ctrl+Shift+F12`) stops all macros and releases everything, also vJoy buttons.
- **Connections**: Soundpad, Discord (mute and deafen), Voicemeeter (gain and mute of strips and
  buses) and vJoy (virtual joysticks fed from virtual controls, mappings and actions), each with
  status, test and gating of its actions.
- **Profiles** per game, switched automatically by the program in the foreground.
- **Overlay**: a transparent, click-through window over borderless games with widgets for
  controls, counters and states, free placement per monitor, themes and appearance settings,
  widgets that show only after a change, and toasts for changes and profile switches.
- **App shell**: tray icon, close to tray, start with Windows, start minimized, single instance,
  "always run as administrator" (with an elevated autostart task) and a warning when the game
  runs as administrator and Trimcraft does not.
- **Diagnostics**: an event log of inputs, triggers, controls, actions and connector events,
  with filters and copying to the clipboard.
- **Backup and transfer**: export and import of the configuration or single profiles (without
  credentials), automatic backups with retention, and restore.
- **Updates**: Trimcraft checks for new releases on startup (can be turned off) and installs
  them from Settings, Updates; releases are signed.

### Changed

- **Renamed from Detent to Trimcraft.** On its first start Trimcraft takes over a Detent
  installation: settings, backups and logs are copied, credentials and autostart are moved,
  and `.detent.json` exports import like `.trimcraft.json` files. Uninstall Detent afterwards.

[Unreleased]: https://github.com/trimcraft/trimcraft/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/trimcraft/trimcraft/releases/tag/v0.1.0
