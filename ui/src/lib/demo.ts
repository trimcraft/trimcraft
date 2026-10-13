// In-browser stand-in for the Rust engine, used only when the UI runs outside Tauri
// (`npm run dev` in a browser). It is deliberately simple: enough to exercise every screen.

import type { Backend } from "./backend";
import type {
  Action,
  DeviceSnapshot,
  DiagEntry,
  DiscordCommand,
  GuideStep,
  EngineSnapshot,
  FunctionDef,
  FunctionView,
  HatDirection,
  KeyInfo,
  MonitorInfo,
  InputMacro,
  MacroStep,
  MouseStep,
  Movement,
  OverlayItem,
  ProcessWindow,
  ProfileTarget,
  PulseStats,
  Radio,
  RecordKind,
  Sound,
  Trigger,
  VirtualControlDef,
  VirtualControlView,
  VoiceState,
} from "./types";
import { CONTROL_TYPES, actionText, controlActions, newTrigger, sameInput, triggerLists } from "./format";

type Cmd = { type: "increment" | "decrement" | "clear" | "toggleOff" } | { type: "select"; index: number };

const HAT_MASKS: Record<number, HatDirection> = {
  1: "up", 3: "upRight", 2: "right", 6: "downRight", 4: "down", 12: "downLeft", 8: "left", 9: "upLeft",
};

// Names of detent-core's action::KEYS (the real app gets them, with scan codes, from Rust).
const KEY_NAMES = [
  ..."ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890",
  ...Array.from({ length: 24 }, (_, i) => `F${i + 1}`),
  "LeftShift", "RightShift", "LeftCtrl", "RightCtrl", "LeftAlt", "RightAlt", "LeftWin", "RightWin", "Menu",
  "Escape", "Tab", "CapsLock", "Space", "Enter", "Backspace", "Insert", "Delete", "Home", "End", "PageUp",
  "PageDown", "Up", "Down", "Left", "Right", "PrintScreen", "ScrollLock", "NumLock",
  ...Array.from({ length: 10 }, (_, i) => `Numpad${i}`),
  "NumpadAdd", "NumpadSubtract", "NumpadMultiply", "NumpadDivide", "NumpadDecimal", "NumpadEnter",
  "Minus", "Equals", "LeftBracket", "RightBracket", "Semicolon", "Apostrophe", "Grave", "Backslash",
  "Comma", "Period", "Slash", "IntlBackslash",
];
const DEMO_KEYS: KeyInfo[] = KEY_NAMES.map((name) => ({ name, scan: 0, extended: false }));

const DEMO_MONITORS: MonitorInfo[] = [
  { name: "DEMO-1", x: 0, y: 0, width: 2560, height: 1440, primary: true, scale: 1 },
  { name: "DEMO-2", x: 2560, y: 0, width: 1920, height: 1080, primary: false, scale: 1 },
];

const DEMO_SOUNDS: Sound[] = ["Siren", "Dog barking", "Duck quack", "Applause", "Drum roll", "Sad trombone"].map(
  (title, i) => ({ index: i + 1, title, path: `C:\\Sounds\\${title.toLowerCase().replace(/ /g, "-")}.mp3`, duration: `0:0${i + 1}` }),
);

/** What the demo "records": a few keys, or a click-drag along a curve (absolute) or a turn (relative). */
function demoRecording(kind: RecordKind, movement: Movement): InputMacro {
  const macro = (steps: MacroStep[]): InputMacro => ({
    type: "inputMacro",
    id: "",
    playback: { speedPercent: 100, minDelayMs: 0, endpointOnly: false },
    movement,
    monitor: null,
    repeat: { type: "once" },
    onEnd: "stopImmediately",
    pauseMs: 0,
    retrigger: "ignore",
    steps,
  });
  const keySteps: MacroStep[] = [
    { type: "keyTap", key: "W", holdMs: 640 },
    { type: "delay", ms: 210 },
    { type: "keyDown", key: "LeftShift" },
    { type: "delay", ms: 90 },
    { type: "keyTap", key: "E", holdMs: 70 },
    { type: "delay", ms: 40 },
    { type: "keyUp", key: "LeftShift" },
  ];
  if (!kind.mouse) return macro(keySteps);
  const n = 14;
  const steps: MouseStep[] =
    movement === "absolute"
      ? [
          { type: "moveTo", x: 0.3, y: 0.6 },
          { type: "delay", ms: 400 },
          { type: "buttonDown", button: "left" },
          { type: "delay", ms: 30 },
          {
            type: "movePath",
            points: Array.from({ length: n }, (_, i) => ({
              x: 0.3 + (0.4 * i) / (n - 1),
              y: 0.6 - 0.3 * Math.sin((Math.PI * i) / (n - 1)),
              t: i * 40,
            })),
          },
          { type: "delay", ms: 60 },
          { type: "buttonUp", button: "left" },
        ]
      : [
          {
            type: "moveRelativePath",
            points: Array.from({ length: n }, (_, i) => ({
              dx: Math.round(600 * Math.sin((Math.PI * i) / (2 * (n - 1)))),
              dy: Math.round(-80 * Math.sin((Math.PI * i) / (n - 1))),
              t: i * 25,
            })),
          },
          { type: "delay", ms: 120 },
          { type: "click", button: "left", holdMs: 60 },
        ];
  // Both: Ctrl held around the mouse part, as a mixed recording looks.
  return macro(kind.keys ? [{ type: "keyDown", key: "LeftCtrl" }, { type: "delay", ms: 120 }, ...steps, { type: "delay", ms: 80 }, { type: "keyUp", key: "LeftCtrl" }] : steps);
}

function device(id: string, name: string, vid: number, pid: number, axes: number, buttons: number, hats: number, virt = false): DeviceSnapshot {
  return {
    id,
    name,
    label: null,
    kind: virt ? { type: "virtual", driver: "vJoy" } : { type: "physical" },
    fingerprint: { vendorId: vid, productId: pid, serial: null, path: `demo://${id}`, name },
    backendGuid: "demo",
    confidence: "path",
    connected: true,
    state: { axes: Array(axes).fill(0), buttons: Array(buttons).fill(false), hats: Array(hats).fill(0) },
  };
}

const stats = (pulses: number): PulseStats => ({
  presses: pulses, pulses, minPulseMs: 28.4, maxPulseMs: 34.9, avgPulseMs: 31.2, longestRun: 5, overlaps: 0, held: false,
});

export function createDemo(): Backend {
  const s: EngineSnapshot = {
    devices: [
      device("f00d:0001#1", "Demo Stick", 0xf00d, 1, 4, 32, 2),
      device("f00d:0002#1", "Demo Throttle", 0xf00d, 2, 3, 24, 1),
      device("f00d:0002#2", "Demo Throttle", 0xf00d, 2, 3, 24, 1),
      device("f00d:0003#1", "Demo Panel", 0xf00d, 3, 0, 16, 0),
      device("1234:bead#1", "vJoy Device", 0x1234, 0xbead, 8, 32, 1, true),
    ],
    lastActivity: null,
    backendError: null,
    encoders: [{ id: "enc-1", name: "Radio knob", device: "f00d:0003#1", decrement: 10, increment: 11, decrementStats: stats(3), incrementStats: stats(4), stepsDown: 0, stepsUp: 0 }],
    suggestions: [],
    halfDetected: [],
    buttonStats: [],
    functions: [],
    profiles: [
      { id: "profile-1", name: "Default", executables: [] },
      { id: "profile-2", name: "DCS World", executables: ["DCS.exe"] },
    ],
    activeProfile: "profile-1",
    profileSwitching: { autoSwitch: true, noMatch: "keepCurrent" },
    foreground: null,
    virtualControls: [],
    learn: null,
    overlayEditing: false,
    recorder: { session: 0, phase: "idle", kind: null, movement: "absolute", result: null, error: null },
    recorderConfig: { stopHotkey: "Ctrl+Shift+F9", tolerance: 2, sampleIntervalMs: 8 },
    connections: {
      soundpad: { enabled: false, timeoutMs: 500, listMaxAgeMs: 10000 },
      discord: { enabled: false, clientId: "", clientSecret: null },
      voicemeeter: { enabled: false },
    },
    connectors: [
      { id: "soundpad", name: "Soundpad", enabled: false, status: { state: "notConfigured" }, detail: null, warning: null, voice: null, credentials: null, guide: null, missing: null },
      { id: "discord", name: "Discord", enabled: false, status: { state: "notConfigured" }, detail: null, warning: null, voice: null, credentials: { secretSaved: false, authorized: false }, guide: null, missing: null },
      { id: "voicemeeter", name: "Voicemeeter", enabled: false, status: { state: "notConfigured" }, detail: null, warning: null, voice: null, credentials: null, guide: null, missing: null },
    ],
    overlay: { visible: true, hotkey: "Ctrl+Shift+F10", monitor: null, anchor: "bottomLeft", margin: 32, scale: 1, widgets: [] },
    detector: { maxPulseMs: 200, maxRunGapMs: 600, minRunLength: 3 },
    settings: { closeToTray: true, startWithWindows: false, startMinimized: true, runAsAdmin: false, trayNoticeShown: false, panicHotkey: "Ctrl+Shift+F12" },
    // Fake elevation: not elevated, and an elevated game in the foreground, so the warning
    // banner shows. "Restart as administrator" clears both.
    shell: { elevated: false, autostart: "off", autostartNote: null, elevatedForeground: "DemoGame.exe" },
  };
  const listeners = new Set<(s: EngineSnapshot) => void>();
  const positions = new Map<string, number>();
  let tick = 0;

  const emit = () => listeners.forEach((l) => l(structuredClone(s)));

  // Discord: a simulated voice state, changed by Discord actions (idempotent, like the app).
  // To see the guide hints: a client ID ending in 0000 is "refused" (App Testers step), a
  // secret of "wrong" is rejected (Client Secret step).
  const demoVoice: VoiceState = { mute: false, deaf: false };
  let demoSecretWrong = false;
  const discordView = () => s.connectors.find((c) => c.id === "discord")!;
  const NO_CREDENTIALS = "Enter the Client ID and Client Secret of your Discord application and save them";
  const NOT_AUTHORIZED = "Not authorized yet: press Test connection and approve the request in Discord";
  function discordStatus(problem: { message: string; guide: GuideStep } | null = null, reported = problem !== null) {
    const c = discordView();
    c.enabled = s.connections.discord.enabled;
    const creds = c.credentials!;
    if (!s.connections.discord.clientId || !creds.secretSaved) problem = { message: NO_CREDENTIALS, guide: "clientId" };
    else if (!creds.authorized && !problem) problem = { message: NOT_AUTHORIZED, guide: "authorize" };
    // Like the app: hints come from a connection attempt (enabled, or Test connection).
    c.guide = c.enabled || reported ? (problem?.guide ?? null) : null;
    if (!c.enabled) Object.assign(c, { status: { state: "notConfigured" }, detail: null, voice: null });
    else if (problem) Object.assign(c, { status: { state: "error", message: problem.message }, detail: null, voice: null });
    else Object.assign(c, { status: { state: "connected" }, detail: "Connected as Demo User", voice: { ...demoVoice } });
  }
  function runDiscord(command: DiscordCommand) {
    if (discordView().status.state !== "connected") return console.info(`[demo] discord: ${command} skipped, not connected`);
    const next = { ...demoVoice };
    if (command === "mute" || command === "unmute") next.mute = command === "mute";
    if (command === "toggleMute") next.mute = !demoVoice.mute;
    if (command === "deafen" || command === "undeafen") next.deaf = command === "deafen";
    if (command === "toggleDeafen") next.deaf = !demoVoice.deaf;
    if (next.mute === demoVoice.mute && next.deaf === demoVoice.deaf) return console.info(`[demo] discord: ${command} skipped, already so`);
    Object.assign(demoVoice, next);
    discordStatus();
  }
  const voiceText = (v: VoiceState) => `mic muted: ${v.mute ? "yes" : "no"}, deafened: ${v.deaf ? "yes" : "no"}`;
  /** "Switch profile" actions, carried out after the trigger (like the engine). */
  const pendingSwitches: ProfileTarget[] = [];
  const runAction = (a: Action) => {
    if (a.type === "discord" && s.connections.discord.enabled) runDiscord(a.command);
    if (a.type === "switchProfile") pendingSwitches.push(a.target);
  };

  // Virtual controls: the semantics of detent-core's virtual_control.rs, simplified: triggers
  // match plain presses and releases (long and double presses, modifiers and axis zones are
  // not simulated); mode conditions are checked. Actions are only logged to the console.
  /** Most recently active state per radio, for toggle off. */
  const lastActive = new Map<string, string>();
  /** Momentary controls: the inputs holding them. Mode shifts: the state to go back to. */
  const held = new Map<string, Trigger[]>();
  const shiftBase = new Map<string, string>();
  const log = (c: VirtualControlView, what: string, actions: Action[]) =>
    actions.forEach((a) => {
      console.info(`[demo] ${c.name} / ${what}:`, actionText(a));
      runAction(a);
    });
  const nextState = (r: Radio, active: number | null, recent: number | null, cmd: Cmd): number | null => {
    const last = r.states.length - 1;
    if (last < 0) return null;
    switch (cmd.type) {
      case "increment":
        return active === null ? 0 : active >= last ? (r.end === "wrap" ? 0 : last) : active + 1;
      case "decrement":
        return active === null ? last : active === 0 ? (r.end === "wrap" ? last : 0) : active - 1;
      case "select":
        if (cmd.index > last) return active;
        return active === cmd.index ? (r.toggleOnReselect ? null : active) : cmd.index;
      case "clear":
        return null;
      case "toggleOff":
        return active !== null ? null : recent !== null && recent <= last ? recent : 0;
    }
  };
  const indexOf = (states: { id: string }[], id: string | null | undefined) => {
    const i = states.findIndex((st) => st.id === id);
    return i >= 0 ? i : null;
  };
  /** Sets the active state of a radio or mode; the state left is remembered. */
  const setActive = (c: VirtualControlView, to: number | null) => {
    if (c.kind.type !== "radio" && c.kind.type !== "mode") return;
    const states = c.kind.states;
    if (c.active !== null && indexOf(states, c.active) !== to) lastActive.set(c.id, c.active);
    c.active = to === null ? null : states[to]!.id;
    c.activeName = to === null ? null : states[to]!.name;
    c.on = c.kind.type === "mode" || to !== null;
  };
  const setCounter = (c: VirtualControlView, value: number) => {
    if (c.kind.type !== "counter") return;
    const { spec } = c.kind;
    const last = Math.floor((spec.max - spec.min) / spec.step + 1e-9);
    const pos = Math.min(Math.max(Math.round((value - spec.min) / spec.step), 0), last);
    c.value = spec.min + pos * spec.step;
    c.display = c.value.toFixed(spec.decimals);
  };
  const runCommand = (c: VirtualControlView, cmd: Cmd) => {
    if (c.kind.type !== "radio") return;
    const r = c.kind;
    const from = indexOf(r.states, c.active);
    const to = nextState(r, from, indexOf(r.states, lastActive.get(c.id)), cmd);
    if (from === to) return;
    if (from !== null) log(c, `${r.states[from]!.name} deactivate`, r.states[from]!.onDeactivate);
    if (to !== null) log(c, `${r.states[to]!.name} activate`, r.states[to]!.onActivate);
    setActive(c, to);
  };
  const setToggle = (c: VirtualControlView, on: boolean) => {
    if (c.kind.type !== "toggle" || c.on === on) return;
    c.on = on;
    c.active = on ? "on" : null;
    c.activeName = on ? "On" : null;
    log(c, on ? "on" : "off", on ? c.kind.onOn : c.kind.onOff);
  };
  const stepCounter = (c: VirtualControlView, by: number | "reset") => {
    if (c.kind.type !== "counter") return;
    const { spec } = c.kind;
    const last = Math.floor((spec.max - spec.min) / spec.step + 1e-9);
    const pos = Math.round(((c.value ?? spec.initial) - spec.min) / spec.step);
    const target = by === "reset" ? Math.round((spec.initial - spec.min) / spec.step) : pos + by;
    const next = spec.end === "wrap" ? ((target % (last + 1)) + last + 1) % (last + 1) : Math.min(Math.max(target, 0), last);
    if (next === pos) return;
    setCounter(c, spec.min + next * spec.step);
    log(c, `value ${c.display}`, c.kind.onChange);
  };
  const goMode = (c: VirtualControlView, to: number) => {
    if (c.kind.type !== "mode") return;
    const from = indexOf(c.kind.states, c.active) ?? 0;
    if (from === to || !c.kind.states[to]) return;
    log(c, `${c.kind.states[from]!.name} deactivate`, c.kind.states[from]!.onDeactivate);
    log(c, `${c.kind.states[to]!.name} activate`, c.kind.states[to]!.onActivate);
    setActive(c, to);
  };
  const stepMode = (c: VirtualControlView, by: number) => {
    if (c.kind.type !== "mode") return;
    const n = c.kind.states.length;
    const i = indexOf(c.kind.states, c.active) ?? 0;
    shiftBase.delete(c.id);
    goMode(c, c.kind.end === "wrap" ? (i + by + n) % n : Math.min(Math.max(i + by, 0), n - 1));
  };
  const setMomentary = (c: VirtualControlView, t: Trigger, down: boolean) => {
    if (c.kind.type !== "momentary") return;
    const list = held.get(c.id) ?? [];
    const was = c.on;
    const rest = list.filter((x) => !sameInput(x, t));
    held.set(c.id, down ? [...rest, t] : rest);
    c.on = (held.get(c.id)!.length > 0) !== c.kind.invert;
    if (c.on !== was) log(c, c.on ? "press" : "release", c.on ? c.kind.onPress : c.kind.onRelease);
  };
  const conditionsHold = (b: Trigger) =>
    b.conditions.every((cond) => {
      const m = allControls().find((x) => x.id === cond.mode);
      return m?.kind.type === "mode" && (m.active ?? m.kind.states[0]?.id) === cond.state;
    });
  /** A press (`down`) or release of `t` through every control of the active profile. */
  const fire = (t: Trigger, down = true) => {
    const matching = (list: Trigger[], kind: "press" | "release" = down ? "press" : "release") =>
      list.some((b) => sameInput(b, t) && b.press.kind === kind && b.modifiers.length === 0 && conditionsHold(b));
    // Every match is decided before anything runs, as in the engine.
    const work: (() => void)[] = [];
    for (const c of s.virtualControls) {
      const k = c.kind;
      switch (k.type) {
        case "radio":
          if (matching(k.increment)) work.push(() => runCommand(c, { type: "increment" }));
          if (matching(k.decrement)) work.push(() => runCommand(c, { type: "decrement" }));
          k.states.forEach((st, index) => { if (matching(st.select)) work.push(() => runCommand(c, { type: "select", index })); });
          if (matching(k.clear)) work.push(() => runCommand(c, { type: "clear" }));
          if (matching(k.toggleOff)) work.push(() => runCommand(c, { type: "toggleOff" }));
          break;
        case "toggle":
          if (matching(k.toggle)) work.push(() => setToggle(c, !c.on));
          if (matching(k.on)) work.push(() => setToggle(c, true));
          if (matching(k.off)) work.push(() => setToggle(c, false));
          break;
        case "momentary":
          if (matching(k.triggers, "press")) work.push(() => setMomentary(c, t, down));
          else if (!down && (held.get(c.id) ?? []).some((x) => sameInput(x, t))) work.push(() => setMomentary(c, t, false));
          break;
        case "counter":
          if (matching(k.increment)) work.push(() => stepCounter(c, 1));
          if (matching(k.decrement)) work.push(() => stepCounter(c, -1));
          if (matching(k.reset)) work.push(() => stepCounter(c, "reset"));
          break;
        case "mode":
          if (matching(k.next)) work.push(() => stepMode(c, 1));
          if (matching(k.previous)) work.push(() => stepMode(c, -1));
          k.states.forEach((st, index) => {
            if (matching(st.select)) work.push(() => { shiftBase.delete(c.id); goMode(c, index); });
            if (down && matching(st.shift, "press")) work.push(() => { if (!shiftBase.has(c.id)) shiftBase.set(c.id, c.active ?? k.states[0]!.id); c.shifted = true; goMode(c, index); });
            if (!down && st.shift.some((b) => sameInput(b, t)) && shiftBase.has(c.id)) work.push(() => {
              goMode(c, indexOf(k.states, shiftBase.get(c.id)) ?? 0);
              shiftBase.delete(c.id);
              c.shifted = false;
            });
          });
          break;
      }
    }
    work.forEach((w) => w());
    runSwitches();
  };
  const findControl = (id: string) => {
    const c = allControls().find((x) => x.id === id);
    if (!c) throw new Error(`unknown virtual control ${id}`);
    return c;
  };

  /** A view of a new control: nothing active, a counter at its initial value, a mode in its first state. */
  function viewOf(def: VirtualControlDef): VirtualControlView {
    const v: VirtualControlView = { ...def, active: null, activeName: null, on: false, value: null, display: null, shifted: false };
    if (def.kind.type === "counter") setCounter(v, def.kind.spec.initial);
    if (def.kind.type === "mode") setActive(v, 0);
    if (def.kind.type === "momentary") v.on = def.kind.invert;
    return v;
  }

  // Profiles: the active one's controls and widgets live in the snapshot, the others here.
  // Control ids are unique across profiles, as in the app.
  const stash = new Map<string, { controls: VirtualControlView[]; widgets: OverlayItem[] }>();
  const demoState = (name: string) => ({ id: "", name, select: [], onActivate: [], onDeactivate: [] });
  stash.set("profile-2", {
    controls: [
      {
        id: "vc-1",
        name: "Radio",
        kind: {
          type: "radio",
          states: ["UHF", "VHF", "Intercom"].map((n, i) => ({ ...demoState(n), id: `st-${i + 1}` })),
          increment: [],
          decrement: [],
          clear: [],
          toggleOff: [],
          end: "wrap",
          toggleOnReselect: false,
          startup: "restoreLast",
        },
        active: "st-2",
        activeName: "VHF",
        on: true,
        value: null,
        display: null,
        shifted: false,
      },
      viewOf({ id: "vc-2", name: "Master arm", kind: { type: "toggle", toggle: [newTrigger({ type: "button", device: "f00d:0002#1", index: 3 })], on: [], off: [], onOn: [], onOff: [], startup: "restoreLast" } }),
      viewOf({
        id: "vc-3",
        name: "COM1",
        kind: {
          type: "counter",
          spec: { min: 118, max: 136.975, step: 0.025, initial: 124.5, end: "wrap", decimals: 3 },
          increment: [newTrigger({ type: "encoderStep", encoder: "enc-1", direction: "increment" })],
          decrement: [newTrigger({ type: "encoderStep", encoder: "enc-1", direction: "decrement" })],
          reset: [{ ...newTrigger({ type: "button", device: "f00d:0003#1", index: 2 }), press: { kind: "long", ms: 800 } }],
          onChange: [],
          startup: "restoreLast",
        },
      }),
      viewOf({
        id: "vc-4",
        name: "Layer",
        kind: {
          type: "mode",
          states: ["Nav", "Combat"].map((n, i) => ({ ...demoState(n), id: `st-${i + 1}`, shift: i === 1 ? [newTrigger({ type: "button", device: "f00d:0002#1", index: 5 })] : [] })),
          next: [],
          previous: [],
          end: "wrap",
          startup: "restoreLast",
        },
      }),
      viewOf({ id: "vc-5", name: "PTT", kind: { type: "momentary", triggers: [newTrigger({ type: "button", device: "f00d:0001#1", index: 0 })], onPress: [], onRelease: [], invert: false } }),
    ],
    widgets: [
      { type: "profileName", position: { x: 0.45, y: 0.02 } },
      { type: "radio", control: "vc-1", layout: "horizontal", position: null },
      { type: "radio", control: "vc-2", layout: "vertical", position: null },
      { type: "radio", control: "vc-3", layout: "vertical", position: null },
      { type: "radio", control: "vc-4", layout: "horizontal", position: null },
      { type: "radio", control: "vc-5", layout: "vertical", position: null },
    ],
  });
  const allControls = () => [...s.virtualControls, ...[...stash.values()].flatMap((p) => p.controls)];
  const allWidgetLists = () => [s.overlay.widgets, ...[...stash.values()].map((p) => p.widgets)];
  const findProfile = (id: string) => {
    const p = s.profiles.find((x) => x.id === id);
    if (!p) throw new Error(`unknown profile ${id}`);
    return p;
  };
  /** Makes a profile active without running actions; its radios keep their states. */
  const activate = (id: string) => {
    if (id === s.activeProfile) return;
    findProfile(id);
    stash.set(s.activeProfile, { controls: s.virtualControls, widgets: s.overlay.widgets });
    const next = stash.get(id) ?? { controls: [], widgets: [] };
    stash.delete(id);
    s.virtualControls = next.controls;
    s.overlay.widgets = next.widgets;
    s.activeProfile = id;
    console.info(`[demo] profile "${findProfile(id).name}" active`);
  };
  function runSwitches() {
    for (const target of pendingSwitches.splice(0)) {
      const n = s.profiles.length;
      const i = s.profiles.findIndex((p) => p.id === s.activeProfile);
      const to =
        target.type === "next" ? s.profiles[(i + 1) % n] : target.type === "previous" ? s.profiles[(i + n - 1) % n] : s.profiles.find((p) => p.id === target.profile);
      if (to) activate(to.id);
    }
  }
  let nextProfile = 3;
  const newProfileId = () => `profile-${nextProfile++}`;
  const DEMO_WINDOWS: ProcessWindow[] = [
    { exe: "DCS.exe", title: "Digital Combat Simulator" },
    { exe: "Discord.exe", title: "#general | Discord" },
    { exe: "explorer.exe", title: "Downloads" },
    { exe: "Il-2.exe", title: "IL-2 Sturmovik: Great Battles" },
    { exe: "Soundpad.exe", title: "Soundpad" },
  ];

  const counterValue = (f: FunctionDef): number => {
    const { spec } = f.kind;
    return spec.min + (positions.get(f.id) ?? 0) * spec.step;
  };
  const view = (f: FunctionDef): FunctionView => {
    const value = counterValue(f);
    return { ...f, value, display: value.toFixed(f.kind.spec.decimals) };
  };
  const refreshFunctions = () => {
    s.functions = s.functions.map(view);
  };
  const step = (encoderId: string, dir: number) => {
    for (const f of s.functions) {
      if (f.kind.input.type !== "encoder" || f.kind.input.encoder !== encoderId) continue;
      const { spec } = f.kind;
      const last = Math.floor((spec.max - spec.min) / spec.step + 1e-9);
      const target = (positions.get(f.id) ?? 0) + dir;
      positions.set(f.id, spec.end === "wrap" ? ((target % (last + 1)) + last + 1) % (last + 1) : Math.min(Math.max(target, 0), last));
    }
    refreshFunctions();
  };

  setInterval(() => {
    tick++;
    const t = tick / 30;
    const [stick, throttle1, throttle2, panel] = s.devices;
    stick!.state.axes[0] = Math.sin(t * 0.9) * 0.8;
    stick!.state.axes[1] = Math.cos(t * 1.3) * 0.6;
    throttle1!.state.axes[2] = Math.sin(t * 0.2);
    throttle2!.state.axes[2] = Math.cos(t * 0.25);
    // Input edges: a trigger and whether it is a press (or a release).
    const edges: [Trigger, boolean][] = [];
    const pressed = Math.floor(tick / 30) % 2 === 1;
    if (pressed !== stick!.state.buttons[0]) edges.push([newTrigger({ type: "button", device: stick!.id, index: 0 }), pressed]);
    stick!.state.buttons[0] = pressed;
    const hat = [0, 1, 2, 4, 8][Math.floor(tick / 45) % 5]!;
    const oldHat = stick!.state.hats[0]!;
    if (hat !== oldHat) {
      if (HAT_MASKS[oldHat]) edges.push([newTrigger({ type: "hat", device: stick!.id, index: 0, direction: HAT_MASKS[oldHat]! }), false]);
      if (HAT_MASKS[hat]) edges.push([newTrigger({ type: "hat", device: stick!.id, index: 0, direction: HAT_MASKS[hat]! }), true]);
    }
    stick!.state.hats[0] = hat;
    // Encoder: a run of 5 detents every 4 s, alternating direction.
    const phase = tick % 120;
    const pulse = phase < 15 && phase % 3 === 0;
    const up = Math.floor(tick / 120) % 2 === 0;
    panel!.state.buttons[10] = pulse && !up;
    panel!.state.buttons[11] = pulse && up;
    if (pulse) {
      for (const e of s.encoders) {
        if (e.device !== panel!.id) continue;
        const button = up ? 11 : 10;
        if (button === e.increment) { e.stepsUp++; step(e.id, 1); edges.push([newTrigger({ type: "encoderStep", encoder: e.id, direction: "increment" }), true]); }
        if (button === e.decrement) { e.stepsDown++; step(e.id, -1); edges.push([newTrigger({ type: "encoderStep", encoder: e.id, direction: "decrement" }), true]); }
      }
      edges.push([newTrigger({ type: "button", device: panel!.id, index: up ? 11 : 10 }), true]);
      s.lastActivity = { device: panel!.id, control: { type: "button", index: up ? 11 : 10 }, timeNs: tick };
    }
    if (edges.length) {
      const press = edges.find(([, down]) => down);
      if (s.learn && !s.learn.captured) { if (press) s.learn.captured = press[0]; }
      else edges.forEach(([t, down]) => fire(t, down));
    }
    emit();
  }, 33);

  // --- Event log: a made-up stream, one entry batch per 100 ms while a window listens -------
  const diagLog: DiagEntry[] = [];
  const diagListeners = new Set<(entries: DiagEntry[]) => void>();
  let diagSeq = 0;
  let diagAxis = 0;
  const pushDiag = (e: Omit<DiagEntry, "seq" | "timeMs" | "level" | "device" | "deviceName" | "detail"> & Partial<DiagEntry>, out: DiagEntry[]) => {
    const entry: DiagEntry = { seq: ++diagSeq, timeMs: Date.now(), level: "info", device: null, deviceName: null, detail: null, ...e };
    diagLog.push(entry);
    if (diagLog.length > 5000) diagLog.shift();
    out.push(entry);
  };
  const STICK = { device: "f00d:0001#1", deviceName: "Demo Stick" };
  const PANEL = { device: "f00d:0003#1", deviceName: "Demo Panel" };
  /** One random "scene" of related entries, like the engine would write for an input. */
  function diagScene(out: DiagEntry[]) {
    const r = Math.random();
    const n = 1 + Math.floor(Math.random() * 16);
    if (r < 0.4) {
      diagAxis = Math.max(-100, Math.min(100, diagAxis + (Math.random() - 0.4) * 12));
      const v = diagAxis.toFixed(1);
      pushDiag({ category: "input", ...STICK, text: `Axis 2: ${diagAxis >= 0 ? "+" : ""}${v} %`, detail: `value ${(diagAxis / 100).toFixed(4)} (range -1..1)` }, out);
    } else if (r < 0.6) {
      pushDiag({ category: "input", ...PANEL, text: `Button ${n} pressed` }, out);
      pushDiag({ category: "trigger", text: `Demo Panel · Button ${n} → Comms: select "Squad"`, detail: "state changed" }, out);
      pushDiag({ category: "control", text: "Comms: Command → Squad", detail: "select \"Squad\"; 1 action(s) queued" }, out);
      pushDiag({ category: "action", text: "Queued: keys: tap F3", detail: "Comms / Squad (activate): keys: tap F3" }, out);
      pushDiag({ category: "action", text: `Executed in ${(2 + Math.random() * 30).toFixed(0)} ms: keys: tap F3`, detail: "Comms / Squad (activate): keys: tap F3" }, out);
      pushDiag({ category: "input", ...PANEL, text: `Button ${n} released` }, out);
    } else if (r < 0.72) {
      const up = Math.random() < 0.5;
      pushDiag({ category: "input", ...PANEL, text: `Encoder Radio knob: detent ${up ? "+" : "−"}`, detail: `from button ${up ? 12 : 11} pressed` }, out);
      pushDiag({ category: "trigger", text: `Radio knob ${up ? "+" : "−"} → Comms: ${up ? "next state" : "previous state"}`, detail: "no change (already in that state or at the end)" }, out);
    } else if (r < 0.82) {
      pushDiag({ category: "input", ...STICK, text: "Hat 1: centered → up-right" }, out);
      pushDiag({ category: "input", ...STICK, text: "Hat 1: up-right → centered" }, out);
    } else if (r < 0.88) {
      pushDiag({ category: "action", text: "Skipped (its connection is disabled): soundpad: play \"Siren\"", level: "warn", detail: "Alarm / On (activate): soundpad: play \"Siren\"" }, out);
    } else if (r < 0.92) {
      pushDiag({ category: "action", text: "Failed (SendInput delivered 0 of 2 inputs (error Access is denied.)): keys: tap F5", level: "error", detail: "Comms / Wing (activate): keys: tap F5" }, out);
    } else if (r < 0.96) {
      pushDiag({ category: "connector", text: "Discord: connected", detail: "Connected as Sam" }, out);
      pushDiag({ category: "connector", text: "Soundpad: error: pipe closed", level: "error" }, out);
    } else {
      pushDiag({ category: "log", text: "could not read Soundpad's sound list: timed out", level: "warn", detail: "log target: detent_app::soundpad" }, out);
    }
  }
  for (let i = 0; i < 12; i++) diagScene([]);
  setInterval(() => {
    if (diagListeners.size === 0) return;
    const batch: DiagEntry[] = [];
    // Mostly a few entries, now and then a burst, like a quickly turned axis or knob.
    const scenes = Math.random() < 0.08 ? 40 : Math.floor(Math.random() * 3);
    for (let i = 0; i < scenes; i++) diagScene(batch);
    if (batch.length) diagListeners.forEach((cb) => cb(structuredClone(batch)));
  }, 100);

  let nextEnc = 2;
  let nextFn = 1;
  let nextVc = 6;
  const ok = async () => emit();

  return {
    meta: async () => ({ version: "0.1.0-demo", backend: "browser demo", dataDir: "–", logDir: "–", platform: "browser", elevated: s.shell.elevated }),
    async subscribe(cb) {
      listeners.add(cb);
      cb(structuredClone(s));
      return () => listeners.delete(cb);
    },
    async setDeviceLabel(id, label) {
      const d = s.devices.find((x) => x.id === id);
      if (d) d.label = label?.trim() || null;
      emit();
    },
    async addEncoder(name, device, decrement, increment) {
      if (decrement === increment) throw new Error("both directions need different buttons");
      const id = `enc-${nextEnc++}`;
      s.encoders.push({ id, name: name.trim() || `Encoder ${nextEnc - 1}`, device, decrement, increment, decrementStats: stats(3), incrementStats: stats(4), stepsDown: 0, stepsUp: 0 });
      s.suggestions = s.suggestions.filter((p) => !(p.device === device && p.decrement === decrement));
      emit();
      return id;
    },
    async renameEncoder(id, name) {
      const e = s.encoders.find((x) => x.id === id);
      if (e && name.trim()) e.name = name.trim();
      emit();
    },
    async swapEncoder(id) {
      const e = s.encoders.find((x) => x.id === id);
      if (e) [e.decrement, e.increment] = [e.increment, e.decrement];
      emit();
    },
    async removeEncoder(id) {
      s.encoders = s.encoders.filter((e) => e.id !== id);
      for (const f of s.functions) if (f.kind.input.type === "encoder" && f.kind.input.encoder === id) f.kind.input = { type: "none" };
      const keep = (t: Trigger) => !(t.type === "encoderStep" && t.encoder === id);
      for (const c of allControls()) for (const list of triggerLists(c.kind)) list.splice(0, list.length, ...list.filter(keep));
      emit();
    },
    async dismissSuggestion(pair) {
      s.suggestions = s.suggestions.filter((p) => !(p.device === pair.device && p.decrement === pair.decrement));
      emit();
    },
    async saveFunction(fn) {
      const { spec } = fn.kind;
      if (!(spec.step > 0)) throw new Error("step must be greater than zero");
      if (!(spec.max > spec.min)) throw new Error("maximum must be greater than minimum");
      const f: FunctionDef = JSON.parse(JSON.stringify({ ...fn, id: fn.id || `fn-${nextFn++}` }));
      if (!fn.id) positions.set(f.id, Math.round((spec.initial - spec.min) / spec.step));
      const i = s.functions.findIndex((x) => x.id === f.id);
      if (i >= 0) s.functions[i] = view(f); else s.functions.push(view(f));
      emit();
      return f.id;
    },
    async removeFunction(id) {
      s.functions = s.functions.filter((f) => f.id !== id);
      for (const list of allWidgetLists()) {
        const i = list.findIndex((w) => w.type === "function" && w.function === id);
        if (i >= 0) list.splice(i, 1);
      }
      emit();
    },
    async setFunctionValue(id, value) {
      const f = s.functions.find((x) => x.id === id);
      if (f) positions.set(id, Math.round((value - f.kind.spec.min) / f.kind.spec.step));
      refreshFunctions();
      emit();
    },
    async toggleOverlayControl(device, control) {
      const i = s.overlay.widgets.findIndex((w) => w.type === "control" && w.device === device && w.control.type === control.type && w.control.index === control.index);
      if (i >= 0) s.overlay.widgets.splice(i, 1); else s.overlay.widgets.push({ type: "control", device, control, label: null, position: null });
      emit();
      return i < 0;
    },
    async toggleOverlayFunction(id) {
      const i = s.overlay.widgets.findIndex((w) => w.type === "function" && w.function === id);
      if (i >= 0) s.overlay.widgets.splice(i, 1); else s.overlay.widgets.push({ type: "function", function: id, position: null });
      emit();
      return i < 0;
    },
    setOverlayLayout: async (layout) => { Object.assign(s.overlay, layout); await ok(); },
    setOverlayVisible: async (visible) => { s.overlay.visible = visible; await ok(); },
    setHotkey: async (hotkey) => { s.overlay.hotkey = hotkey; await ok(); },
    setDetectorConfig: async (config) => { s.detector = config; await ok(); },
    keys: async () => DEMO_KEYS,
    async saveVirtualControl(control) {
      const c = structuredClone(control);
      const k = c.kind;
      const states = k.type === "radio" || k.type === "mode" ? k.states : [];
      if ((k.type === "radio" || k.type === "mode") && !states.length) throw new Error("a radio needs at least one state");
      if (k.type === "counter" && !(k.spec.step > 0)) throw new Error("step must be greater than zero");
      if (k.type === "counter" && !(k.spec.max > k.spec.min)) throw new Error("maximum must be greater than minimum");
      const actions = controlActions(k);
      let nextMacro = 1 + Math.max(0, ...actions.flatMap((a) => (a.type === "inputMacro" ? [Number(a.id.replace("mac-", "")) || 0] : [])));
      for (const a of actions) if (a.type === "inputMacro" && !a.id) a.id = `mac-${nextMacro++}`;
      for (const step of actions.flatMap((a) => (a.type === "inputMacro" ? a.steps : []))) {
        if ("key" in step) {
          const known = KEY_NAMES.find((n) => n.toLowerCase() === step.key.trim().toLowerCase());
          if (!known) throw new Error(`unknown key "${step.key}"`);
          step.key = known;
        }
      }
      for (const a of actions) {
        if (a.type === "soundpad" && a.command.type === "play" && !a.command.sound.title.trim() && !a.command.sound.path.trim()) {
          throw new Error("choose a Soundpad sound to play");
        }
        if (a.type === "switchProfile" && a.target.type === "profile") findProfile(a.target.profile);
      }
      const held = k.type === "momentary" ? k.triggers : k.type === "mode" ? k.states.flatMap((st) => st.shift) : [];
      if (held.some((t) => t.type === "encoderStep" || t.press.kind === "release")) {
        throw new Error("held controls (momentary, mode shift) need a button, hat or axis zone pressed, not a release or encoder");
      }
      let nextState = Math.max(0, ...states.map((st) => Number(st.id.replace("st-", "")) || 0)) + 1;
      states.forEach((st, i) => {
        if (!st.id) st.id = `st-${nextState++}`;
        st.name = st.name.trim() || `State ${i + 1}`;
      });
      c.name = c.name.trim() || CONTROL_TYPES.find(([t]) => t === k.type)![1];
      if (!c.id) {
        c.id = `vc-${nextVc++}`;
        s.virtualControls.push(viewOf(c));
        s.overlay.widgets.push({ type: "radio", control: c.id, layout: "vertical", position: null });
      } else {
        const i = s.virtualControls.findIndex((x) => x.id === c.id);
        if (i < 0) throw new Error(`unknown virtual control ${c.id}`);
        const old = s.virtualControls[i]!;
        const v = viewOf(c);
        // Editing keeps the state where it still exists and never runs actions.
        if (k.type === "radio" || k.type === "mode") {
          const still = states.findIndex((st) => st.id === old.active);
          if (still >= 0 || k.type === "radio") setActive(v, still >= 0 ? still : null);
        }
        if (k.type === "toggle") Object.assign(v, { on: old.on, active: old.active, activeName: old.activeName });
        if (k.type === "counter" && old.value !== null) setCounter(v, old.value);
        s.virtualControls[i] = v;
        if (!states.some((st) => st.id === lastActive.get(c.id))) lastActive.delete(c.id);
        shiftBase.delete(c.id);
      }
      emit();
      return c.id;
    },
    async removeVirtualControl(id) {
      lastActive.delete(id);
      shiftBase.delete(id);
      s.virtualControls = s.virtualControls.filter((c) => c.id !== id);
      s.overlay.widgets = s.overlay.widgets.filter((w) => !(w.type === "radio" && w.control === id));
      emit();
    },
    async setRadioActive(id, state) {
      const c = findControl(id);
      if (c.kind.type === "toggle") {
        Object.assign(c, { on: state !== null, active: state === null ? null : "on", activeName: state === null ? null : "On" });
      } else if (c.kind.type === "radio" || c.kind.type === "mode") {
        const i = indexOf(c.kind.states, state);
        if (state !== null && i === null) throw new Error(`unknown state ${state}`);
        setActive(c, c.kind.type === "mode" ? (i ?? 0) : i);
        shiftBase.delete(id);
        c.shifted = false;
      } else throw new Error("this command does not apply to this kind of control");
      emit();
    },
    async commandRadio(id, command) {
      const c = findControl(id);
      const k = c.kind;
      if (k.type === "radio" && (command === "increment" || command === "decrement" || command === "clear" || command === "toggleOff")) runCommand(c, { type: command });
      else if (k.type === "toggle" && (command === "toggle" || command === "on" || command === "off")) setToggle(c, command === "toggle" ? !c.on : command === "on");
      else if (k.type === "counter" && (command === "increment" || command === "decrement" || command === "reset")) stepCounter(c, command === "reset" ? "reset" : command === "increment" ? 1 : -1);
      else if (k.type === "mode" && (command === "increment" || command === "decrement")) stepMode(c, command === "increment" ? 1 : -1);
      else throw new Error("this command does not apply to this kind of control");
      runSwitches();
      emit();
    },
    async setControlValue(id, value) {
      const c = findControl(id);
      if (c.kind.type !== "counter" || !Number.isFinite(value)) throw new Error("this command does not apply to this kind of control");
      setCounter(c, value);
      emit();
    },
    async toggleOverlayVirtualControl(id) {
      const i = s.overlay.widgets.findIndex((w) => w.type === "radio" && w.control === id);
      if (i >= 0) s.overlay.widgets.splice(i, 1); else s.overlay.widgets.push({ type: "radio", control: id, layout: "vertical", position: null });
      emit();
      return i < 0;
    },
    startLearn: async () => { s.learn = { captured: null }; await ok(); },
    stopLearn: async () => { s.learn = null; await ok(); },
    listMonitors: async () => DEMO_MONITORS,
    async setOverlayMonitor(monitor) {
      s.overlay.monitor = monitor;
      await ok();
    },
    async setOverlayWidget(index, item) {
      if (!s.overlay.widgets[index]) throw new Error(`overlay widget ${index} does not exist`);
      s.overlay.widgets[index] = structuredClone(item);
      await ok();
    },
    async setOverlayPositions(positions) {
      if (positions.length !== s.overlay.widgets.length) throw new Error("widget count changed");
      const plain: typeof positions = JSON.parse(JSON.stringify(positions));
      s.overlay.widgets.forEach((w, i) => (w.position = plain[i] ?? null));
      await ok();
    },
    setOverlayEditing: async (editing) => { s.overlayEditing = editing; await ok(); },
    async setConnections(connections) {
      const sp = connections.soundpad;
      if (sp.timeoutMs < 100 || sp.timeoutMs > 5000) throw new Error("timeout must be between 100 and 5000 ms");
      const enabling = sp.enabled && !s.connections.soundpad.enabled;
      // Client ID and secret reference only change through saveDiscordCredentials.
      const discord = { ...s.connections.discord, enabled: connections.discord.enabled };
      s.connections = { ...structuredClone(connections), discord };
      const c = s.connectors[0]!;
      c.enabled = sp.enabled;
      if (!sp.enabled) Object.assign(c, { status: { state: "notConfigured" }, detail: null, warning: null });
      else if (enabling) {
        c.status = { state: "connecting" };
        setTimeout(() => {
          if (s.connections.soundpad.enabled) Object.assign(c, { status: { state: "connected" }, detail: `${DEMO_SOUNDS.length} sounds` });
          emit();
        }, 800);
      }
      // Voicemeeter: "installed" and running in the demo.
      const vm = s.connectors.find((x) => x.id === "voicemeeter")!;
      vm.enabled = connections.voicemeeter.enabled;
      Object.assign(vm, vm.enabled ? { status: { state: "connected" }, detail: "Voicemeeter Banana (demo)" } : { status: { state: "notConfigured" }, detail: null });
      discordStatus();
      emit();
    },
    async testConnection(id) {
      if (id === "soundpad") return `Connected, ${DEMO_SOUNDS.length} sounds (demo)`;
      if (id === "voicemeeter") return "Connected to Voicemeeter Banana (demo)";
      const creds = discordView().credentials!;
      if (!s.connections.discord.clientId || !creds.secretSaved) {
        discordStatus(null, true);
        emit();
        throw new Error(NO_CREDENTIALS);
      }
      if (!creds.authorized) {
        Object.assign(discordView(), { status: { state: "waitingForApproval" }, detail: "Approve the request in your Discord client", guide: null });
        emit();
        await new Promise((r) => setTimeout(r, 1500));
        const problem: { message: string; guide: GuideStep } | null = s.connections.discord.clientId.endsWith("0000")
          ? { message: "Discord refused the RPC scopes for this application. Unapproved applications may only use RPC for their testers: in the Developer Portal, App Testers, add your own Discord account and accept the invitation (code 5000: OAuth2 Error: invalid_scope) (demo)", guide: "appTesters" }
          : demoSecretWrong
            ? { message: "Discord rejected the Client Secret (or Client ID): copy both again from the Developer Portal, OAuth2 page (invalid_client) (demo)", guide: "clientSecret" }
            : null;
        if (problem) {
          discordStatus(problem);
          emit();
          throw new Error(problem.message);
        }
        creds.authorized = true;
      }
      discordStatus();
      emit();
      return `Connected as Demo User, ${voiceText(demoVoice)} (demo)`;
    },
    async saveDiscordCredentials(clientId, clientSecret) {
      if (!/^\d{15,22}$/.test(clientId.trim())) throw new Error("the Client ID is a long number; copy it from the Developer Portal (OAuth2 page)");
      const creds = discordView().credentials!;
      if (!clientSecret?.trim() && !creds.secretSaved) throw new Error("enter the Client Secret");
      if (clientId.trim() !== s.connections.discord.clientId) creds.authorized = false;
      if (clientSecret?.trim()) demoSecretWrong = clientSecret.trim() === "wrong";
      creds.secretSaved = true;
      s.connections.discord = { ...s.connections.discord, clientId: clientId.trim(), clientSecret: { store: "windowsCredentialManager", target: "detent/discord/client-secret" } };
      discordStatus();
      emit();
    },
    async forgetDiscordAuthorization() {
      discordView().credentials!.authorized = false;
      discordStatus();
      emit();
    },
    async removeDiscordCredentials() {
      discordView().credentials = { secretSaved: false, authorized: false };
      s.connections.discord = { ...s.connections.discord, clientId: "", clientSecret: null };
      discordStatus();
      emit();
    },
    discordSetup: async () => ({ redirectUri: "https://localhost/", scopes: ["rpc", "rpc.voice.read", "rpc.voice.write"], portalUrl: "https://discord.com/developers/applications" }),
    async openDiscordPortal() {
      window.open("https://discord.com/developers/applications", "_blank", "noopener");
    },
    soundpadSounds: async () => structuredClone(DEMO_SOUNDS),
    async startRecording(kind, movement) {
      if (s.recorder.phase !== "idle") throw new Error("a recording is already running");
      const session = s.recorder.session + 1;
      s.recorder = { session, phase: "countdown", kind, movement, result: null, error: null };
      setTimeout(() => {
        if (s.recorder.session === session && s.recorder.phase === "countdown") s.recorder.phase = "recording";
        emit();
      }, 3000);
      emit();
    },
    async stopRecording() {
      const r = s.recorder;
      if (r.phase === "recording" && r.kind) r.result = { session: r.session, action: demoRecording(r.kind, r.movement), notes: ["demo recording"] };
      r.phase = "idle";
      emit();
    },
    cancelRecording: async () => { s.recorder.phase = "idle"; await ok(); },
    clearRecording: async () => { s.recorder.result = null; s.recorder.error = null; await ok(); },
    async setRecorderConfig(config) {
      if (!config.stopHotkey.trim() || /esc/i.test(config.stopHotkey)) throw new Error(`hotkey "${config.stopHotkey}" is not valid`);
      s.recorderConfig = structuredClone(config);
      await ok();
    },
    async setSettings(settings) {
      s.settings = { ...structuredClone(settings), trayNoticeShown: s.settings.trayNoticeShown };
      // Like detent-core's plan_autostart: the task needs administrator rights.
      const { startWithWindows, runAsAdmin } = s.settings;
      const elevated = s.shell.elevated;
      s.shell.autostart = !startWithWindows ? "off" : runAsAdmin && elevated ? "task" : "runKey";
      s.shell.autostartNote =
        startWithWindows && runAsAdmin && !elevated
          ? "Registered without administrator rights for now (UAC prompt at logon); Detent switches to a Task Scheduler task at the next start as administrator."
          : null;
      await ok();
    },
    async restartElevated() {
      s.shell.elevated = true;
      s.shell.elevatedForeground = null;
      if (s.settings.startWithWindows) {
        s.shell.autostart = s.settings.runAsAdmin ? "task" : "runKey";
        s.shell.autostartNote = null;
      }
      await ok();
    },
    async selectProfile(id) {
      activate(id);
      emit();
    },
    async createProfile(name) {
      const id = newProfileId();
      s.profiles.push({ id, name: name.trim() || `Profile ${s.profiles.length + 1}`, executables: [] });
      activate(id);
      emit();
      return id;
    },
    async duplicateProfile(id, name) {
      const source = findProfile(id);
      const data = id === s.activeProfile ? { controls: s.virtualControls, widgets: s.overlay.widgets } : (stash.get(id) ?? { controls: [], widgets: [] });
      const renamed = new Map<string, string>();
      const controls = structuredClone(data.controls).map((c) => {
        const newId = `vc-${nextVc++}`;
        renamed.set(c.id, newId);
        const last = lastActive.get(c.id);
        if (last) lastActive.set(newId, last);
        return { ...c, id: newId };
      });
      const widgets = structuredClone(data.widgets).map((w) => (w.type === "radio" ? { ...w, control: renamed.get(w.control) ?? w.control } : w));
      const copy = { id: newProfileId(), name: name.trim() || `${source.name} copy`, executables: [] };
      s.profiles.push(copy);
      stash.set(copy.id, { controls, widgets });
      activate(copy.id);
      emit();
      return copy.id;
    },
    async renameProfile(id, name) {
      const p = findProfile(id);
      if (name.trim()) p.name = name.trim();
      emit();
    },
    async deleteProfile(id) {
      findProfile(id);
      if (s.profiles.length === 1) throw new Error("the last profile cannot be deleted");
      s.profiles = s.profiles.filter((p) => p.id !== id);
      if (id === s.activeProfile) {
        const first = s.profiles[0]!.id;
        const next = stash.get(first) ?? { controls: [], widgets: [] };
        stash.delete(first);
        s.virtualControls = next.controls;
        s.overlay.widgets = next.widgets;
        s.activeProfile = first;
      } else stash.delete(id);
      emit();
    },
    async makeDefaultProfile(id) {
      const p = findProfile(id);
      s.profiles = [p, ...s.profiles.filter((x) => x.id !== id)];
      emit();
    },
    async setProfileExecutables(id, executables) {
      const p = findProfile(id);
      const clean: string[] = [];
      for (const raw of executables) {
        const name = raw.trim().replace(/^"|"$/g, "").split(/[\\/]/).pop()!.trim();
        if (!name || /[<>:"|?*]/.test(name)) throw new Error(`"${raw}" is not a program name; enter e.g. DCS.exe`);
        const exe = name.includes(".") ? name : `${name}.exe`;
        if (!clean.some((e) => e.toLowerCase() === exe.toLowerCase())) clean.push(exe);
      }
      p.executables = clean;
      emit();
    },
    setProfileSwitching: async (switching) => { s.profileSwitching = { ...switching }; await ok(); },
    async toggleOverlayProfileName() {
      const i = s.overlay.widgets.findIndex((w) => w.type === "profileName");
      if (i >= 0) s.overlay.widgets.splice(i, 1); else s.overlay.widgets.push({ type: "profileName", position: null });
      emit();
      return i < 0;
    },
    listProcessWindows: async () => structuredClone(DEMO_WINDOWS),
    async testAction(action) {
      const macro = action.type === "inputMacro";
      setTimeout(() => console.info("[demo] test play:", actionText(action)), macro ? 3000 : 0);
    },
    listVoices: async () => [
      { id: "demo/zira", name: "Microsoft Zira Desktop - English (United States)" },
      { id: "demo/hedda", name: "Microsoft Hedda - German (Germany)" },
    ],
    listSoundDevices: async () => ["Speakers (Realtek Audio)", "Headphones (Demo Headset)"],
    listAudioPrograms: async () => ["chrome.exe", "Discord.exe", "DCS.exe"],
    async storeHttpSecret(value) {
      if (!value) throw new Error("enter the secret value");
      // The demo keeps no secrets; only the reference comes back.
      return { store: "windowsCredentialManager", target: `detent/http/demo-${Date.now().toString(16)}` };
    },
    async subscribeDiagnostics(cb) {
      diagListeners.add(cb);
      cb(structuredClone(diagLog));
      return () => diagListeners.delete(cb);
    },
    async clearDiagnostics() {
      diagLog.length = 0;
    },
    async openLogFolder() {
      throw new Error("not available in the browser demo");
    },
  };
}
