# Detent

Configuration tool for HOTAS controllers (stick, throttle, pedals, button boxes) on Windows.

> A *detent* is the tactile notch on a throttle lever where it clicks into place.

## Goals

- Detect all connected game controllers and show every axis, button and hat live.
- Map physical inputs to named, per-game **functions** (e.g. "Landing gear"), and functions to
  outputs: key presses, a virtual joystick (vJoy), macros, or mode/shift-layer changes.
- Per-game profiles, switched automatically when the game starts.
- In-game **overlay** showing the state of buttons, axes and functions, with a layout designer.

## Status

Concept phase. No code yet.

## Planned stack

- Core: Rust
- UI and overlay: Tauri 2 (TypeScript frontend)
- Input: SDL3 behind an `InputBackend` abstraction
- Output: `SendInput`, vJoy, HidHide (Windows only)
- Profiles: JSON, one file per game

## Platform

Runs on Windows 10/11. Core logic, input and UI are developed and tested on macOS as well;
Windows-only outputs are stubbed there.
