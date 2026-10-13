<script lang="ts">
  // Editor for one input macro: keyboard and mouse steps in any order. Playback options,
  // repetition, recording (keys, mouse or both; append or replace), test play, and the step
  // list (select, drag to reorder, insert, delete, edit in place). Macros with mouse movement
  // also get a preview of it with draggable waypoints.
  import { onMount, untrack } from "svelte";
  import type {
    EngineSnapshot,
    InputMacro,
    KeyInfo,
    KeyStep,
    MacroStep,
    MonitorInfo,
    MonitorRef,
    MouseButton,
    MouseStep,
    RecordingResult,
    Repeat,
  } from "../lib/types";
  import { isKeyStep, isMouseStep } from "../lib/format";
  import { backend, errorText } from "../lib/backend";
  import { COUNTDOWN_MS, countdown, newEditorId, recordingOwner, reproject, type RecordMode } from "../lib/recording.svelte";
  import PathPreview from "./PathPreview.svelte";

  let {
    action = $bindable(),
    keys,
    snapshot,
  }: { action: InputMacro; keys: KeyInfo[]; snapshot: EngineSnapshot } = $props();

  const id = newEditorId();

  let error = $state("");
  let notes = $state<string[]>([]);
  let mode = $state<RecordMode>("append");
  let selected = $state<number[]>([]);
  let focus = $state<number | null>(null);
  let monitors = $state<MonitorInfo[]>([]);
  let testing = $state(false);
  const testCountdown = countdown(() => testing);

  /** What the next recording takes. */
  let recordKeys = $state(true);
  let recordMouse = $state(true);
  let hasMouse = $derived(action.steps.some(isMouseStep));
  /** Mouse options matter when the macro has mouse steps or will record some. */
  let mouseOptions = $derived(hasMouse || recordMouse);
  let recordingWhat = $derived.by(() => {
    const kind = snapshot.recorder.kind;
    return kind ? [kind.keys && "keys", kind.mouse && "mouse"].filter(Boolean).join(" and ") : "";
  });
  let recorder = $derived(snapshot.recorder);
  let mine = $derived(recordingOwner.id === id);
  let busy = $derived(recorder.phase !== "idle");
  const recCountdown = countdown(() => mine && recorder.phase === "countdown");

  onMount(() => {
    backend()
      .then((b) => b.listMonitors())
      .then((m) => (monitors = m))
      .catch(() => {});
  });

  // --- Recording ---------------------------------------------------------------------------

  /** This editor's recording was seen running, so going idle without a result is a cancel. */
  let sawActive = false;

  $effect(() => {
    const r = snapshot.recorder;
    if (recordingOwner.id !== id) return;
    if (r.result) {
      const result = $state.snapshot(r.result);
      untrack(() => take(result));
      release(true);
    } else if (r.error) {
      error = r.error;
      release(true);
    } else if (r.phase !== "idle") {
      sawActive = true;
    } else if (sawActive) {
      release(false);
    }
  });

  // Leaving the editor while recording cancels the recording.
  $effect(() => () => {
    if (recordingOwner.id === id) {
      recordingOwner.id = null;
      backend().then((b) => b.cancelRecording());
    }
  });

  function release(clear: boolean) {
    recordingOwner.id = null;
    sawActive = false;
    if (clear) backend().then((b) => b.clearRecording());
  }

  const sameMonitor = (a: MonitorRef, b: MonitorRef) => a.name === b.name && a.x === b.x && a.y === b.y;
  const hasAbsolute = (steps: MacroStep[]) => steps.some((s) => s.type === "moveTo" || s.type === "movePath");

  /** The monitor the macro's absolute moves refer to, with its size (saved, or the primary one). */
  let macroMonitor = $derived.by((): MonitorRef | null => action.monitor ?? monitors.find((m) => m.primary) ?? null);

  function take(result: RecordingResult) {
    const rec = result.action;
    const replace = recordingOwner.mode === "replace";
    notes = result.notes;
    selected = [];
    focus = null;
    let steps = rec.steps;
    if (hasAbsolute(steps) && rec.monitor) {
      if (replace || !hasAbsolute(action.steps)) {
        action.monitor = rec.monitor;
      } else if (macroMonitor && !sameMonitor(macroMonitor, rec.monitor)) {
        // Appending a recording made on another monitor: keep one reference monitor.
        steps = reproject(steps, rec.monitor, macroMonitor);
      }
    }
    action.steps = replace ? steps : [...action.steps, ...steps];
    if (result.notes.length === 0 && rec.steps.length === 0) notes = ["Nothing was recorded."];
  }

  async function record() {
    error = "";
    notes = [];
    recordingOwner.id = id;
    recordingOwner.mode = mode;
    sawActive = false;
    try {
      await (await backend()).startRecording({ keys: recordKeys, mouse: recordMouse }, action.movement);
    } catch (e) {
      recordingOwner.id = null;
      error = errorText(e);
    }
  }

  const stop = async () => (await backend()).stopRecording();
  const cancel = async () => (await backend()).cancelRecording();

  async function testPlay() {
    error = "";
    try {
      await (await backend()).testAction($state.snapshot(action));
      testing = true;
      setTimeout(() => (testing = false), COUNTDOWN_MS);
    } catch (e) {
      error = errorText(e);
    }
  }

  // --- Steps -------------------------------------------------------------------------------

  const mouseButtons: MouseButton[] = ["left", "right", "middle", "x1", "x2"];
  const keyStepTypes: [KeyStep["type"], string][] = [
    ["keyTap", "Tap key"],
    ["keyDown", "Press key"],
    ["keyUp", "Release key"],
  ];
  const mouseStepTypes: [MouseStep["type"], string][] = [
    ["click", "Click"],
    ["buttonDown", "Press button"],
    ["buttonUp", "Release button"],
    ["wheel", "Wheel"],
    ["moveRelative", "Move by"],
    ["moveTo", "Move to"],
  ];
  const pathTypes: [MouseStep["type"], string][] = [
    ["movePath", "Path"],
    ["moveRelativePath", "Relative path"],
  ];
  const speeds = [25, 50, 75, 100, 150, 200, 300, 400];

  function knownKey(name: string): boolean {
    const n = name.trim().toLowerCase();
    return keys.length === 0 || keys.some((k) => k.name.toLowerCase() === n);
  }

  /** Changes a step's type (also between keyboard and mouse), keeping the key or button where
   * it makes sense. */
  function stepAs(step: MacroStep, type: MacroStep["type"]): MacroStep {
    const key = "key" in step ? step.key : "F1";
    const button = "button" in step ? step.button : "left";
    switch (type) {
      case "keyTap":
        return { type, key, holdMs: null };
      case "keyDown":
      case "keyUp":
        return { type, key };
      case "click":
        return { type, button, holdMs: null };
      case "buttonDown":
      case "buttonUp":
        return { type, button };
      case "wheel":
        return { type, delta: 1 };
      case "moveRelative":
        return { type, dx: 0, dy: 0 };
      case "moveTo":
        return { type, x: 0.5, y: 0.5 };
      case "delay":
        return { type, ms: 100 };
      case "movePath":
      case "moveRelativePath":
        // Paths come from recordings; they cannot be made from another step.
        return step;
    }
  }

  /** A new step: like the one above it (key or mouse), a key tap in an empty macro. */
  function newStep(above: MacroStep | undefined): MacroStep {
    return above && isMouseStep(above) ? { type: "click", button: "left", holdMs: null } : { type: "keyTap", key: "", holdMs: null };
  }

  function insertAfter(i: number) {
    action.steps.splice(i + 1, 0, newStep(action.steps[i]));
    selected = [];
  }

  // --- Repetition ----------------------------------------------------------------------------

  function setRepeat(type: Repeat["type"]) {
    action.repeat = type === "times" ? { type, count: action.repeat.type === "times" ? action.repeat.count : 3 } : { type };
  }
  let background = $derived(action.repeat.type === "whileActive" || (action.repeat.type === "times" && action.repeat.count > 1));

  function removeAt(i: number) {
    action.steps.splice(i, 1);
    selected = [];
    if (focus === i) focus = null;
  }

  function toggle(i: number) {
    selected = selected.includes(i) ? selected.filter((x) => x !== i) : [...selected, i];
  }

  function deleteSelected() {
    const drop = new Set(selected);
    action.steps = action.steps.filter((_, i) => !drop.has(i));
    selected = [];
    focus = null;
  }

  function moveStep(from: number, to: number) {
    if (from === to) return;
    const steps = action.steps;
    const [s] = steps.splice(from, 1);
    if (s) steps.splice(to, 0, s);
    selected = [];
    focus = null;
  }

  // Drag to reorder with pointer events (HTML drag and drop is not available in the app's
  // webview on Windows).
  let rows: HTMLElement[] = $state([]);
  let drag = $state<{ from: number; to: number } | null>(null);

  function dragStart(e: PointerEvent, i: number) {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { from: i, to: i };
  }

  function dragMove(e: PointerEvent) {
    if (!drag) return;
    let to = drag.to;
    rows.forEach((row, i) => {
      if (!row) return;
      const r = row.getBoundingClientRect();
      if (e.clientY >= r.top && e.clientY < r.bottom) to = i;
    });
    drag.to = to;
  }

  function dragEnd() {
    if (drag) moveStep(drag.from, drag.to);
    drag = null;
  }

  /** Duration of a path: its last waypoint's time. Changing it rescales all waypoint times. */
  function pathDuration(points: { t: number }[]): number {
    return points.at(-1)?.t ?? 0;
  }

  function setPathDuration(points: { t: number }[], ms: number) {
    const old = pathDuration(points);
    if (old <= 0 || ms < 0) return;
    for (const p of points) p.t = Math.round((p.t * ms) / old);
  }

  const holdValue = (v: number | null) => (v === null ? "" : String(v));
  const parseHold = (v: string): number | null => (v.trim() === "" ? null : Math.max(0, Math.round(Number(v)) || 0));
  const int = (v: string) => Math.round(Number(v)) || 0;
  const percent = (v: number) => Math.round(v * 1000) / 10;
  const fromPercent = (v: string) => (Number(v) || 0) / 100;

  const monitorKey = (m: MonitorRef | null) => (m ? `${m.name}@${m.x},${m.y}` : "");
  function setMonitor(key: string) {
    const m = monitors.find((x) => monitorKey(x) === key);
    action.monitor = m ? { name: m.name, x: m.x, y: m.y, width: m.width, height: m.height } : null;
  }
</script>

<div class="macro">
  <div class="row options">
    <label>Speed
      <select bind:value={action.playback.speedPercent}>
        {#each speeds.includes(action.playback.speedPercent) ? speeds : [...speeds, action.playback.speedPercent].sort((a, b) => a - b) as s (s)}
          <option value={s}>{s === 100 ? "original" : `${s / 100}×`}</option>
        {/each}
      </select>
    </label>
    <label title="Recorded waits shorter than this are skipped when playing">Skip waits under
      <input type="number" min="0" max="60000" value={action.playback.minDelayMs} onchange={(e) => (action.playback.minDelayMs = Math.max(0, int(e.currentTarget.value)))} /> ms
    </label>
    {#if mouseOptions}
      <label title="How the recorder captures movement: cursor positions, or raw deltas for mouse-look">Movement
        <select bind:value={action.movement}>
          <option value="absolute">absolute (cursor position)</option>
          <option value="relative">relative (mouse-look)</option>
        </select>
      </label>
      <label title="Absolute positions refer to this monitor">Monitor
        <select value={monitorKey(action.monitor)} onchange={(e) => setMonitor(e.currentTarget.value)}>
          <option value="">primary</option>
          {#each monitors as m (monitorKey(m))}
            <option value={monitorKey(m)}>{m.name ?? "monitor"} {m.width}×{m.height}{m.primary ? " (primary)" : ""}</option>
          {/each}
          {#if action.monitor && !monitors.some((m) => monitorKey(m) === monitorKey(action.monitor))}
            <option value={monitorKey(action.monitor)}>{action.monitor.name ?? "monitor"} (not connected)</option>
          {/if}
        </select>
      </label>
      <label class="check" title="Absolute paths jump straight to their end"><input type="checkbox" bind:checked={action.playback.endpointOnly} /> endpoint only</label>
    {/if}
  </div>

  <div class="row repeat">
    <label title="How often the macro runs when its trigger fires">Repeat
      <select value={action.repeat.type} onchange={(e) => setRepeat(e.currentTarget.value as Repeat["type"])}>
        <option value="once">once</option>
        <option value="times">n times</option>
        <option value="whileActive">while active</option>
      </select>
    </label>
    {#if action.repeat.type === "times"}
      <input type="number" min="1" max="10000" value={action.repeat.count} onchange={(e) => action.repeat.type === "times" && (action.repeat.count = Math.max(1, int(e.currentTarget.value)))} /> times
    {/if}
    {#if action.repeat.type === "whileActive"}
      <label title="What happens when the control state that started the macro ends">When the state ends
        <select bind:value={action.onEnd}>
          <option value="stopImmediately">stop immediately</option>
          <option value="finishRun">finish the current run</option>
        </select>
      </label>
    {/if}
    {#if action.repeat.type !== "once"}
      <label title="Pause between two runs">Pause <input type="number" min="0" max="600000" value={action.pauseMs} onchange={(e) => (action.pauseMs = Math.max(0, int(e.currentTarget.value)))} /> ms</label>
    {/if}
    <label title="What the trigger does while this macro still runs">If triggered while running
      <select bind:value={action.retrigger}>
        <option value="ignore">ignore</option>
        <option value="restart">restart</option>
      </select>
    </label>
  </div>
  {#if background}
    <p class="muted small note">
      Runs in the background: the next actions do not wait for it.
      {action.repeat.type === "whileActive"
        ? "It repeats as long as the state that started it stays active (a momentary held, a toggle on, a radio or mode state active)."
        : ""}
      Stopping it (state ended, Stop macros, panic hotkey {snapshot.settings.panicHotkey}) releases the keys and buttons it holds.
    </p>
  {/if}

  <div class="row record">
    {#if mine && recorder.phase === "countdown"}
      <span class="rec">● Recording starts in {recCountdown.left}… switch to the game</span>
      <button onclick={cancel}>Cancel</button>
    {:else if mine && recorder.phase === "recording"}
      <span class="rec live">● Recording {recordingWhat} — stop with {snapshot.recorderConfig.stopHotkey} or Stop, Esc cancels</span>
      <button class="primary" onclick={stop}>Stop</button>
      <button onclick={cancel}>Cancel</button>
    {:else}
      <button class="recbtn" disabled={busy || (!recordKeys && !recordMouse)} onclick={record} title="Records after a {COUNTDOWN_MS / 1000} s countdown">● Record</button>
      <label class="check" title="Record keys"><input type="checkbox" bind:checked={recordKeys} disabled={busy} /> keys</label>
      <label class="check" title="Record mouse buttons, wheel and movement"><input type="checkbox" bind:checked={recordMouse} disabled={busy} /> mouse</label>
      <select bind:value={mode} disabled={busy} title="What the recording does with the existing steps">
        <option value="append">append to steps</option>
        <option value="replace">replace steps</option>
      </select>
      <button disabled={testing || action.steps.length === 0} onclick={testPlay} title="Plays the macro as edited after a {COUNTDOWN_MS / 1000} s countdown">
        {testing ? `▶ Playing in ${testCountdown.left}…` : "▶ Test play"}
      </button>
      {#if busy && !mine}<span class="muted small">Another macro is being recorded.</span>{/if}
    {/if}
  </div>
  {#if error}<p class="error small">{error}</p>{/if}
  {#each notes as note (note)}<p class="muted small note">{note}</p>{/each}

  <div class="row selection">
    <span class="muted small">{action.steps.length} steps</span>
    {#if selected.length}
      <button class="small" onclick={deleteSelected}>Delete {selected.length} selected</button>
      <button class="small" onclick={() => (selected = [])}>Clear selection</button>
    {:else if action.steps.length > 1}
      <button class="small" onclick={() => (selected = action.steps.map((_, i) => i))}>Select all</button>
    {/if}
  </div>

  <div class="steps">
    {#each action.steps as step, si (si)}
      <div
        class="row step"
        class:sel={selected.includes(si)}
        class:focused={focus === si}
        class:target={drag !== null && drag.to === si && drag.from !== si}
        class:above={drag !== null && drag.to < drag.from}
        bind:this={rows[si]}
      >
        <input type="checkbox" checked={selected.includes(si)} onchange={() => toggle(si)} title="Select" />
        <span class="handle" title="Drag to reorder" role="presentation" onpointerdown={(e) => dragStart(e, si)} onpointermove={dragMove} onpointerup={dragEnd} onpointercancel={() => (drag = null)}>⋮⋮</span>
        <span class="n muted">{si + 1}</span>
        <span class="kind muted" title={isKeyStep(step) ? "keyboard" : isMouseStep(step) ? "mouse" : "wait"}>{isKeyStep(step) ? "⌨" : isMouseStep(step) ? "🖱" : "⏱"}</span>
        <select value={step.type} onchange={(e) => (action.steps[si] = stepAs(step, e.currentTarget.value as MacroStep["type"]))}>
          <optgroup label="Keyboard">{#each keyStepTypes as [value, text] (value)}<option {value}>{text}</option>{/each}</optgroup>
          <optgroup label="Mouse">
            {#each mouseStepTypes as [value, text] (value)}<option {value}>{text}</option>{/each}
            {#each pathTypes.filter(([v]) => v === step.type) as [value, text] (value)}<option {value}>{text}</option>{/each}
          </optgroup>
          <option value="delay">Wait</option>
        </select>
        {#if isKeyStep(step)}
          {@const ks = step as KeyStep}
          {#if ks.type !== "delay"}
            <input class="key mono" class:bad={!knownKey(ks.key)} list="trimcraft-keys" bind:value={ks.key} placeholder="key" />
            {#if ks.type === "keyTap"}
              <label class="muted">hold <input type="number" min="0" max="60000" placeholder="50" value={holdValue(ks.holdMs)} onchange={(e) => (ks.holdMs = parseHold(e.currentTarget.value))} /> ms</label>
            {/if}
          {/if}
        {:else}
          {@const ms = step as MouseStep}
          {#if ms.type === "delay"}
            <input type="number" min="0" max="60000" value={ms.ms} onchange={(e) => (ms.ms = Math.max(0, int(e.currentTarget.value)))} /> ms
          {:else if ms.type === "wheel"}
            <input type="number" value={ms.delta} onchange={(e) => (ms.delta = int(e.currentTarget.value))} /> <span class="muted">notches (+ up, − down)</span>
          {:else if ms.type === "moveRelative"}
            <label class="muted">dx <input type="number" value={ms.dx} onchange={(e) => (ms.dx = int(e.currentTarget.value))} /></label>
            <label class="muted">dy <input type="number" value={ms.dy} onchange={(e) => (ms.dy = int(e.currentTarget.value))} /></label>
          {:else if ms.type === "moveTo"}
            <label class="muted">x <input type="number" step="0.1" value={percent(ms.x)} onchange={(e) => (ms.x = fromPercent(e.currentTarget.value))} /> %</label>
            <label class="muted">y <input type="number" step="0.1" value={percent(ms.y)} onchange={(e) => (ms.y = fromPercent(e.currentTarget.value))} /> %</label>
            <button class="small link" onclick={() => (focus = focus === si ? null : si)}>{focus === si ? "hide" : "show"}</button>
          {:else if ms.type === "movePath" || ms.type === "moveRelativePath"}
            <span class="muted">{ms.points.length} waypoints,</span>
            <label class="muted">duration <input type="number" min="0" max="60000" value={pathDuration(ms.points)} onchange={(e) => setPathDuration(ms.points, int(e.currentTarget.value))} /> ms</label>
            <button class="small link" onclick={() => (focus = focus === si ? null : si)}>{focus === si ? "hide" : "show"}</button>
          {:else}
            <select bind:value={ms.button}>
              {#each mouseButtons as b (b)}<option value={b}>{b}</option>{/each}
            </select>
            {#if ms.type === "click"}
              <label class="muted">hold <input type="number" min="0" max="60000" placeholder="50" value={holdValue(ms.holdMs)} onchange={(e) => (ms.holdMs = parseHold(e.currentTarget.value))} /> ms</label>
            {/if}
          {/if}
        {/if}
        <span class="spacer"></span>
        <button class="small" title="Insert a step below" onclick={() => insertAfter(si)}>+</button>
        <button class="small" title="Delete" onclick={() => removeAt(si)}>×</button>
      </div>
    {/each}
    <button class="small add" onclick={() => insertAfter(action.steps.length - 1)}>Add step</button>
  </div>

  {#if action.steps.some((s) => ["moveTo", "movePath", "moveRelativePath", "moveRelative"].includes(s.type))}
    <PathPreview bind:steps={() => action.steps as MouseStep[], (v) => (action.steps = v)} monitor={macroMonitor} {focus} />
  {/if}
</div>

<style>
  .macro { display: flex; flex-direction: column; gap: 6px; width: 100%; font-size: 12px; }
  .options label { display: inline-flex; gap: 4px; align-items: center; color: var(--muted); }
  .check { cursor: pointer; }
  .rec { color: var(--bad); font-weight: 600; }
  .rec.live { animation: blink 1s steps(2, start) infinite; }
  @keyframes blink { to { opacity: 0.55; } }
  .recbtn { color: var(--bad); border-color: var(--bad); }
  .recbtn:disabled, button:disabled { opacity: 0.5; cursor: default; }
  .note { margin: 0; }
  .steps { display: flex; flex-direction: column; gap: 3px; align-items: flex-start; }
  .step { width: 100%; padding: 1px 2px; border-radius: 3px; border-top: 2px solid transparent; border-bottom: 2px solid transparent; }
  .step.sel { background: var(--panel); }
  .step.focused { outline: 1px solid var(--accent); }
  .step.target { border-bottom-color: var(--accent); }
  .step.target.above { border-bottom-color: transparent; border-top-color: var(--accent); }
  .handle { cursor: grab; color: var(--muted); user-select: none; padding: 0 2px; touch-action: none; }
  .n { width: 2.2em; text-align: right; }
  .key { width: 10em; }
  .kind { width: 1.4em; text-align: center; }
  .repeat label { display: inline-flex; gap: 4px; align-items: center; color: var(--muted); }
  .bad { border-color: var(--bad); }
  .spacer { flex: 1; }
  .small { font-size: 12px; }
  .link { border: none; background: none; color: var(--accent); }
  label { display: inline-flex; gap: 4px; align-items: center; }
</style>
