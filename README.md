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

The main window has three sections: **Virtual Controls** (opened by default), **Connections** and
**Hardware Test** (everything above).

Hardware Test has two tabs. **Devices** is the live view of all controls. **Event log** records what Detent
sees and does, newest at the bottom: button, hat and encoder input (axes only when they moved at least
2 %, at most 10 entries per second per axis), trigger matches, state changes of virtual controls,
actions (queued, executed with duration, skipped, failed), connector status changes, and warnings and
errors from the app log. The last 5000 entries are kept in memory. Filter by category, device or text,
pause the view while recording continues, copy what is visible, or open the log folder. The file log
(`%LOCALAPPDATA%oud.breu.detentogsdetent.log`) is unchanged.
A virtual control is built from physical inputs and can run actions. The first kind is the
**radio**: several states of which at most one is active, like the preset buttons of a car radio.

- Triggers: a button press, a hat direction being entered, or one encoder detent. Next state,
  previous state, "all off", "toggle off" and "select this state" each take any number of
  triggers. Assign them by pressing the control ("learn") or pick them from a list. Toggle off
  turns the active state off, or brings back the most recently active one (the first state if
  none was active yet); this also works after a restart.
- Each state has actions on activate and on deactivate. Switching from A to B runs A's
  deactivate actions, then B's activate actions.
- Actions are input macros: keyboard steps (key down, key up, tap with hold time, wait) and mouse steps
  (button down, up, click, wheel, relative move, move to a position, recorded paths, wait),
  Soundpad commands, Discord mute/deafen, switching the profile (see Profiles), and the actions
  below. Keys are sent as scan codes so games that read DirectInput or RawInput see them. Every
  executed action is logged. Add actions with **+ Add action…**.
- **Soundpad** actions play a sound (on speakers, on the mic, or both), stop, pause/resume, or
  play a random, the next or the previous sound. Pick the sound from Soundpad's live list; it is
  stored by title and file path, so rearranging the list in Soundpad does not break it. Soundpad
  (full version, with its remote control) must be running, and the Soundpad connection must be
  enabled under **Connections**.
- **Discord** actions mute, unmute, deafen, undeafen, or toggle mute or deafen. Mute and deafen
  set the state (nothing is sent when it already holds); the toggles go by Discord's live state,
  so muting by hand in Discord is taken into account. Discord must be running and the Discord
  connection enabled under **Connections**.
- **Speak text**: Windows speech with a voice of your choice (the installed Windows voices),
  rate and volume. A new speech interrupts the one still speaking.
- **Play sound file**: wav, mp3, ogg or flac, with volume, on the default or a chosen output
  device; optionally stops the sounds still playing first.
- **Audio volume**: the master volume of the default output device, or a program's volume (by
  executable name, e.g. `Discord.exe`, like the Windows volume mixer; the list offers programs
  that have an audio session now): set to %, change by ±%, mute, unmute, toggle mute.
- **Media key**: play/pause, next, previous, stop, volume up/down, mute, as a keyboard's media
  keys.
- **Launch**: start a program (path, arguments, working folder, optionally only if it is not
  running yet), open a web page (http/https only) or a file or folder with its default app.
  Programs are started directly, never through a command prompt.
- **HTTP request** (webhook): method, address, headers and body, timeout (default 5 s). Header
  values can be stored as **secret**: they go to the Windows Credential Manager
  (`detent/http/…`) and never into `setup.json` or the log. The status and duration go to the
  event log.
- **Voicemeeter** (a connection, see below): set or change the gain of a strip or bus, mute,
  unmute or toggle it.
- Texts of speech and HTTP requests can contain placeholders: `{control}`, `{state}`,
  `{profile}` and `{value}` (the control's value, for controls that have one; empty for
  radios). In an HTTP address their values are URL-encoded.
- Actions run one after the other, in order. Speech, sound files, launch and HTTP requests start
  in the background and the next action runs at once, unless **Wait until finished** is ticked
  (it waits at most 10 minutes). Their result, and any failure with its reason, goes to the event
  log; a failing action never stops the ones after it. **▶ Test** runs an action as edited.
- At the ends a radio wraps around or stops; selecting the active state again can turn it off;
  at startup the last state is restored (without running actions) or none is active.
- On the overlay a radio shows its name and all its states, vertically or horizontally, with the
  active state highlighted.

### More control kinds

**New …** offers five kinds; each lives in a profile, can go on the overlay and keeps its state
across restarts (unless set to start empty).

- **Toggle**: on or off. Triggers toggle, switch on, switch off; actions on switching on and off.
  Overlay: name and an ON/OFF lamp.
- **Momentary**: on only while a trigger is held (a button, hat direction or axis zone); off when
  the last one is released, even if the profile changed meanwhile. Actions on press and on
  release; **invert** makes it on while nothing is held.
- **Counter**: a value with minimum, maximum, step, wrap or stop and decimals (like the Hardware
  Test counters, which stay as they are). Triggers increment, decrement and reset (encoders,
  buttons, hats, zones); actions on every change can use the new value as `{value}`.
- **Mode** (layer): like a radio with exactly one state active, the first by default, no "all
  off". States are selected latching (select, next, previous) or as a **shift**: while a
  trigger is held the mode is in that state, on release it goes back. A latching change while
  shifted ends the shift.

### Triggers

Click a trigger in the editor to set more than the input:

- **Press kind** (buttons and hat directions): press (default), release, **long press** (held at
  least 500 ms, configurable) or **double press** (second press within 300 ms, configurable). If a
  button has a long or double press trigger in the active profile, its plain presses fire on
  release instead (a "short press"; with a double press, once the window has passed), so a long
  press does not also fire the short one. The editor marks such triggers *short*. Momentary
  controls and shifts always react to the press itself.
- **Modifiers**: buttons (any device) that must be held. The most specific trigger wins: with
  `Shift + Button 4` assigned, Button 4 alone does not fire while Shift is held.
- **Only when**: one or more conditions "Mode X is in state S" (all must hold).
- **Axis zones**: assign by moving an axis (a quarter of its range); the zone (0–100 %) is set on
  a live axis bar. It fires on entering, optionally on leaving, with 2 % hysteresis so jitter at
  the edge does not fire again. Where the axis is at startup counts as the start.

Long and double presses are timed from the input's own timestamps; Detent wakes up exactly when
one is due instead of polling.

### Recording macros

An **input macro** has keyboard and mouse steps in any order, e.g. Ctrl down, click, Ctrl up
(the keyboard and mouse macros of earlier versions load as input macros with all their
settings). Every input macro has a **● Record** button. After a 3-second countdown (switch
to the game now) Detent records into that macro until you press the stop hotkey (default
`Ctrl+Shift+F9`, configurable under *Macro recorder*) or **Stop**; **Esc** cancels. While
recording, the main window shows a red banner and the overlay a red **REC** badge. The recording
is appended to the steps or replaces them, as chosen next to the button. The stop hotkey's keys
and the click on Stop are not recorded; Detent's own playback never is (injected input is
ignored). Esc cannot be recorded; add it by hand if a macro needs it.

- Tick what to record: **keys** (as scan codes, like playback), **mouse** (buttons, wheel and
  movement), or both (the default). Gaps become waits, and press-wait-release becomes a tap or
  click. Recording both puts keys and mouse on one timeline; a mouse path during which a key
  goes down or up is split there, so a key held while moving the mouse stays in time.
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
- The editor has a step list (⌨ keyboard, 🖱 mouse, ⏱ wait; select several to delete, drag ⋮⋮
  to reorder, + inserts a step, everything editable in place, a step can change between key
  and mouse) and, when the macro moves the mouse, a preview of the movement on the monitor
  outline where waypoints can be dragged. **▶ Test play** plays the macro as edited after the
  same countdown ("while active" plays once there).

### Repeating and stopping macros

- **Repeat**: once (default), **n times**, or **while active**: as long as the control state
  that started the macro stays active (a momentary control held, a toggle on, a radio or mode
  state active; for a long-press trigger on a momentary control, while the button is held). A
  **pause** between runs can be set. For a "while active" macro, when the state ends it stops
  immediately (default) or finishes the run in progress.
- A macro that runs once plays in order with the other actions, as before. A repeating macro
  runs in the background; the next actions do not wait for it.
- The same macro never runs twice at once: a new trigger while it runs is **ignored**
  (default) or **restarts** it.
- Stopping a macro releases every key and mouse button it pressed and has not released, so
  nothing stays stuck. Macros stop when their state ends, with the **Stop macros** action (all,
  or one macro of the profile), with the **panic hotkey** (Settings, General; default
  `Ctrl+Shift+F12`, works while a game has the focus), when another profile becomes active and
  when Detent quits. The panic hotkey, a profile switch and quitting also release what macros
  that already finished left pressed (e.g. a "Ctrl down" waiting for its "Ctrl up"). The
  panic hotkey then also releases every vJoy button and centres the POVs (axes stay); they
  stay released until a control changes, a vJoy action runs or the profile changes. On quit,
  macros stop and release first, then vJoy releases its buttons and gives the devices back.
- The event log shows a background macro's start, and for every stop the reason, the number of
  runs and the keys released.

Relative moves are sent as relative mouse input: games that read raw input see exactly the
recorded deltas, while the Windows desktop applies its pointer speed and acceleration to them.

Windows does not deliver synthesised input to programs running with higher privileges: if a
game runs as administrator, Detent must too for its macros to reach the game (and for the
recorder to see input while the game has focus).

Mapping to a virtual joystick comes in a later stage.

## Profiles

A **profile** belongs to one game: it has its own radios (virtual controls), its own overlay
widgets with their positions and layout, and its own active radio states. Devices, encoders,
counters, connections, the recorder settings and the overlay's monitor, size, hotkey and
visibility are shared by all profiles. A setup from before profiles becomes the profile
**Default**.

- The **Profile** dropdown in the header shows the active profile and switches it by hand.
  **Manage profiles…** (or the top of **Virtual Controls**) lists all profiles: create, rename,
  duplicate (a full copy of radios, widgets and states, without the programs), delete (not the
  last one), and **Make default** (the first profile is the default one).
- Each profile lists the **programs** that make it active, e.g. `DCS.exe` (case does not matter;
  a full path keeps only the file name, a name without extension gets `.exe`). Type them in or
  **Pick from running processes** (programs with a window, with their titles).
- **Auto-switch** (on by default): when a program a profile lists comes to the front, that
  profile becomes active. Detent watches the foreground window event-driven
  (`SetWinEventHook`), without polling. **When no game matches**: *Keep current profile*
  (default; alt-tabbing to Discord keeps the game's profile) or *Switch to the default
  profile*. Detent's own windows never switch profiles. If several profiles list the same
  program, the active one stays, otherwise the first in the list wins.
- The action **Switch profile** (to a profile, the next or the previous one) lets a controller
  button switch profiles. A profile chosen by hand (dropdown or action) stays until a program
  that a profile lists comes to the front; another window of the same program does not count.
- Switching **never runs actions**: every radio keeps its state per profile and is shown again
  as it was; the overlay updates at once. Each switch is logged.
- The overlay widget **Profile name** (button *Show profile name* under Overlay) shows the
  active profile's name; like all widgets it is set per profile.

## Connections

The **Connections** section lists the programs Detent can control: Soundpad, Discord and
Voicemeeter. Each
connection can be enabled, has its own settings (for Soundpad: timeout per request, how old the
cached sound list may get), a live status (disabled, connecting, connected, not reachable) and a
**Test connection** button. While enabled, Detent checks the connection every few seconds and
reconnects with a growing pause when the program is gone; the number of sounds is shown.

A connection's actions are offered in the action editor only while it is enabled. Actions of a
connection that is later disabled stay saved, are shown greyed with a link to Connections, and
are skipped (with a log entry) when they fire. Keyboard and mouse macros need no connection.
Connection settings are stored in `setup.json`; secrets are kept in the Windows Credential
Manager (generic credentials `detent/discord/…`), never in the file or the log.

### Voicemeeter

Uses the VoicemeeterRemote API of a local Voicemeeter (Standard, Banana or Potato). Detent loads
`VoicemeeterRemote64.dll` from the installation folder; without an installation the connection
says so and its actions are not offered. Voicemeeter must be running; while the connection is
enabled, Detent checks every few seconds and shows the edition.

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

### vJoy

Detent can feed virtual joysticks from [vJoy](https://github.com/BrunnerInnovation/vJoy), which
games see as ordinary controllers. vJoy must be installed; Detent loads its
`vJoyInterface.dll` at run time. The vJoy card under **Connections** shows the driver version and
the configured devices 1–16 with their axes, buttons and POVs and who owns each (free, Detent,
another program). Axes, buttons and POVs are set up in **Configure vJoy** (vJoyConf), not in
Detent; a fresh vJoy has one device with three axes and no buttons, so add buttons and POVs
there before using button or POV outputs (Detent warns when an output needs more than a device
has).

While the connection is enabled, Detent acquires only the vJoy devices the active profile uses
and gives them back when it is disabled, when the profile no longer uses them, and on exit;
before giving a device back it releases its buttons and centres its POVs. Everything below is
per profile:

- **State outputs** (in the control editor, "vJoy"): radio and mode states, toggles and
  momentary controls hold a vJoy button while active, or press it briefly ("pulsed on
  entering", e.g. 100 ms) for games that want one press per state. A counter can drive a vJoy
  axis: minimum at one end, maximum at the other.
- **vJoy mappings** (Virtual Controls page): a physical axis to a vJoy axis with invert,
  deadzones (centre and both ends), saturation (largest output) and a response curve (linear or
  expo with a factor), with a live graph of the curve and the current position; physical
  buttons and hats to vJoy buttons and POVs. Every input event is written as it happens.
- **vJoy actions**: press (held), release, toggle or pulse a button; set an axis to a percentage;
  point a POV or centre it. They take effect when the action fires, not after waits of earlier
  macros, and hold until another action changes them, the profile changes or vJoy is disabled.

On a profile switch everything the old profile held is released and the new profile's outputs
apply at once (also at startup, with the restored states). vJoy devices never act as triggers
or mapping sources, so Detent's own output cannot loop back. Output changes appear in the
event log under *vJoy output* (axes at most 10 times per second each).

To see the output: open **vJoy Monitor** (`JoyMonitor.exe` in the vJoy folder, or "Monitor
vJoy" in the Start menu) and pick the device, or open Windows' game controller settings
(`joy.cpl`), select "vJoy Device" and press *Properties*.

## Tray and settings

Detent keeps a **tray icon** while it runs: a left click opens the main window; its menu has
*Show window*, *Overlay visible* (the same switch as the overlay hotkey) and *Quit*. Starting
Detent a second time opens the running instance instead.

The gear in the header opens **Settings**. Under *General*:

- **Close button minimizes to the tray** (on by default): closing the window only hides it;
  inputs, actions and the overlay keep running. Quit from the tray menu. The first time,
  Windows shows a notification that Detent is still running.
- **Start with Windows** (off by default): registered per user. A start by Windows always
  goes to the tray.
- **Start minimized to the tray** (on by default): also for starts by hand. The very first
  start (no settings saved yet) always shows the window.
- **Always run as administrator** (off by default): Windows does not let a normal program send
  keys and mouse input to a program that runs as administrator. If a game does, Detent must too.
  With this on, Detent restarts itself as administrator (UAC prompt) when started normally.
  Together with *Start with Windows* it is registered as a Task Scheduler task with highest
  privileges instead of a startup entry, so there is no prompt at logon. Creating or removing
  that task needs administrator rights; until Detent runs as administrator it uses the normal
  startup entry. **Restart as administrator** does it right away.

The settings page and the version tooltip show whether Detent runs as administrator. When it
does not and the program in the foreground does, a banner in the main window warns that
Detent's keys and mouse will not reach it (also in the log).

Uninstalling removes the startup entry and, after a UAC prompt, the task (only if it exists).

## Backup and transfer

Settings has a section **Backup & transfer**.

- **Export** writes the configuration, or selected profiles, to a `.detent.json` file (also per profile in the profile
  list). The file holds the schema version and the export time. Credentials are never exported: the Discord client secret
  reference and any field named like a secret, token, password or authorization header are emptied, and the file lists
  what was left out. After importing, enter those under Connections.
- **Import** reads such a file (or a plain `setup.json`). Older schemas are converted; files from a newer Detent are
  refused with a message. **Replace all** swaps the whole configuration (this computer's tray and startup settings stay,
  and so do the Discord credentials when the file names the same Client ID) after a confirmation. **Add profiles** adds the
  chosen profiles: names that are taken get a number ("DCS (2)"), ids are renumbered where they clash, the encoders and
  counters the profiles use come along, and programs that another profile already lists are not taken over. Before
  either, a backup of the current configuration is written; if that fails, nothing is imported.
- **Automatic backups**: whenever `setup.json` is saved, at most every 5 minutes, and before every import and restore,
  Detent copies it to `backups\setup-YYYYMMDD-HHMMSS.json` (UTC) in the config folder. It keeps the latest 20 and the
  newest of each of the last 14 days. The list shows date, profile count and size; **Restore** (after a confirmation,
  with a backup of the current state first), **Back up now** and **Open backup folder** are there too. Backup events appear
  in the event log.

## Overlay toasts and appearance

- **Toasts**: every virtual control has "Show toast on change" (off by default) in its editor. When its state or value
  changes, a toast shows the name and the new state, e.g. "COM1 · 121.500" or "Radio · Kanal 2": it fades in and out, at
  most 3 are stacked with the newest on top, and a repeated change of the same control updates its toast instead of
  adding one. Switching the profile shows "Profile: <name>" (can be turned off). Duration (default 2 s), the profile
  toast and "Show toasts while the overlay is hidden" are under Settings > Overlay appearance. With the overlay hidden by
  the hotkey, only the toasts appear; the window stays click-through and is shown without activation, so a game keeps
  the keyboard and mouse. The toast area is placed in Edit layout like a widget (default: top centre) and belongs to the
  profile. "Show a test toast" in Settings tries it.
- **Widget visibility**: in the Overlay panel each widget can be "Always" or "Only after a change" (shown for N seconds,
  default 3, then it fades out), and can have its own size and opacity on top of the overlay's. In layout edit mode all
  widgets are visible.
- **Appearance** (Settings > Overlay appearance, all profiles): theme (Dark, Light, High contrast, HUD green), accent
  color for active states, background opacity, text size and corner radius, with a live preview; the overlay window
  follows while you adjust. The highlight of the active state never changes a widget's size in any theme.

## Layout

| Path | What |
|---|---|
| `crates/detent-core` | Platform-independent model: device identity, live state, encoders, counters, virtual controls, actions, overlay layout. All logic is here and unit-tested. |
| `crates/detent-input` | Input backends: SDL3 and a mock with synthetic devices. |
| `src-tauri` | Desktop app (Tauri 2): engine thread, windows, hotkey, persistence. |
| `ui` | Svelte 5 + TypeScript frontend: main window and overlay window. |

## Development

Detent runs on Windows 10/11 only, and is developed there. Requirements: Rust (stable, MSVC),
Node 24, CMake (SDL3 is built from source and linked statically).

```sh
npm install
npx tauri dev                       # app with real devices
DETENT_INPUT=mock npx tauri dev     # app with synthetic devices (no hardware needed)
npm run dev                         # UI only, in a browser, with an in-page demo
cargo run -p detent-input --example probe -- 30   # print raw input events for 30 s
./scripts/check.sh                  # full gate, same as CI; run before every push
```

Settings live in `%APPDATA%\cloud.breu.detent`: `devices.json` (known devices and
labels), `setup.json` (encoders, counters, profiles with their virtual controls and overlay widgets, overlay settings, recorder, connections,
app-wide settings), `values.json`
(current counter values), `control-states.json` (active radio states of all profiles).

## CI

Every push to `main` runs the gate and builds the app on Windows. The build
is downloadable as the `detent-windows` artifact (NSIS installer and plain `detent.exe`).
Builds are not code-signed; Windows SmartScreen will warn on first start.
