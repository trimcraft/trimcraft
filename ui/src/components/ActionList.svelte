<script lang="ts">
  // Edits a list of actions (keyboard and mouse macros) and their steps.
  import type { Action, KeyInfo, KeyStep, MouseButton, MouseStep } from "../lib/types";
  import { actionText } from "../lib/format";
  import SoundpadEditor from "./SoundpadEditor.svelte";

  let { label, actions = $bindable(), keys }: { label: string; actions: Action[]; keys: KeyInfo[] } = $props();

  let open = $state<number | null>(null);

  const mouseButtons: MouseButton[] = ["left", "right", "middle", "x1", "x2"];
  const keyStepTypes: [KeyStep["type"], string][] = [
    ["keyTap", "Tap"],
    ["keyDown", "Press"],
    ["keyUp", "Release"],
    ["delay", "Wait"],
  ];
  const mouseStepTypes: [MouseStep["type"], string][] = [
    ["click", "Click"],
    ["buttonDown", "Press"],
    ["buttonUp", "Release"],
    ["wheel", "Wheel"],
    ["moveRelative", "Move"],
    ["delay", "Wait"],
  ];

  function knownKey(name: string): boolean {
    const n = name.trim().toLowerCase();
    return keys.length === 0 || keys.some((k) => k.name.toLowerCase() === n);
  }

  function newAction(type: Action["type"]): Action {
    switch (type) {
      case "keyboardMacro":
        return { type, steps: [{ type: "keyTap", key: "F1", holdMs: null }] };
      case "mouseMacro":
        return { type, steps: [{ type: "click", button: "left", holdMs: null }] };
      case "soundpad":
        return { type, command: { type: "play", sound: { title: "", path: "" }, output: "both" } };
    }
  }

  function addAction(type: Action["type"]) {
    actions.push(newAction(type));
    open = actions.length - 1;
  }

  function move<T>(list: T[], i: number, by: number) {
    const j = i + by;
    if (j < 0 || j >= list.length) return;
    [list[i], list[j]] = [list[j]!, list[i]!];
  }

  /** Changes a step's type, keeping the key or button where it makes sense. */
  function keyStepAs(step: KeyStep, type: KeyStep["type"]): KeyStep {
    const key = "key" in step ? step.key : "F1";
    switch (type) {
      case "keyTap":
        return { type, key, holdMs: null };
      case "keyDown":
      case "keyUp":
        return { type, key };
      case "delay":
        return { type, ms: 100 };
    }
  }

  function mouseStepAs(step: MouseStep, type: MouseStep["type"]): MouseStep {
    const button = "button" in step ? step.button : "left";
    switch (type) {
      case "click":
        return { type, button, holdMs: null };
      case "buttonDown":
      case "buttonUp":
        return { type, button };
      case "wheel":
        return { type, delta: 1 };
      case "moveRelative":
        return { type, dx: 0, dy: 0 };
      case "delay":
        return { type, ms: 100 };
    }
  }

  const holdValue = (v: number | null) => (v === null ? "" : String(v));
  const parseHold = (v: string): number | null => (v.trim() === "" ? null : Math.max(0, Math.round(Number(v)) || 0));
  const int = (v: string) => Math.round(Number(v)) || 0;
</script>

<div class="actions">
  <span class="label">{label}</span>
  <div class="body">
    {#each actions as action, ai (ai)}
      <div class="action">
        <div class="row head">
          <button class="link" onclick={() => (open = open === ai ? null : ai)}>{open === ai ? "▾" : "▸"} {actionText(action)}</button>
          <span class="spacer"></span>
          <button class="small" title="Move up" onclick={() => move(actions, ai, -1)}>↑</button>
          <button class="small" title="Move down" onclick={() => move(actions, ai, 1)}>↓</button>
          <button class="small" onclick={() => { actions.splice(ai, 1); open = null; }}>Delete</button>
        </div>
        {#if open === ai}
          <div class="steps">
            {#if action.type === "keyboardMacro"}
              {#each action.steps as step, si (si)}
                <div class="row step">
                  <span class="n muted">{si + 1}</span>
                  <select value={step.type} onchange={(e) => (action.steps[si] = keyStepAs(step, e.currentTarget.value as KeyStep["type"]))}>
                    {#each keyStepTypes as [value, text] (value)}<option {value}>{text}</option>{/each}
                  </select>
                  {#if step.type === "delay"}
                    <input type="number" min="0" max="60000" value={step.ms} onchange={(e) => (step.ms = Math.max(0, int(e.currentTarget.value)))} /> ms
                  {:else}
                    <input class="key mono" class:bad={!knownKey(step.key)} list="detent-keys" bind:value={step.key} placeholder="key" />
                    {#if step.type === "keyTap"}
                      <label class="muted">hold <input type="number" min="0" max="60000" placeholder="50" value={holdValue(step.holdMs)} onchange={(e) => (step.holdMs = parseHold(e.currentTarget.value))} /> ms</label>
                    {/if}
                  {/if}
                  <span class="spacer"></span>
                  <button class="small" onclick={() => move(action.steps, si, -1)}>↑</button>
                  <button class="small" onclick={() => move(action.steps, si, 1)}>↓</button>
                  <button class="small" onclick={() => action.steps.splice(si, 1)}>×</button>
                </div>
              {/each}
              <button class="small" onclick={() => action.steps.push({ type: "keyTap", key: "", holdMs: null })}>Add step</button>
            {:else if action.type === "mouseMacro"}
              {#each action.steps as step, si (si)}
                <div class="row step">
                  <span class="n muted">{si + 1}</span>
                  <select value={step.type} onchange={(e) => (action.steps[si] = mouseStepAs(step, e.currentTarget.value as MouseStep["type"]))}>
                    {#each mouseStepTypes as [value, text] (value)}<option {value}>{text}</option>{/each}
                  </select>
                  {#if step.type === "delay"}
                    <input type="number" min="0" max="60000" value={step.ms} onchange={(e) => (step.ms = Math.max(0, int(e.currentTarget.value)))} /> ms
                  {:else if step.type === "wheel"}
                    <input type="number" value={step.delta} onchange={(e) => (step.delta = int(e.currentTarget.value))} /> <span class="muted">detents (+ up, − down)</span>
                  {:else if step.type === "moveRelative"}
                    <label class="muted">dx <input type="number" value={step.dx} onchange={(e) => (step.dx = int(e.currentTarget.value))} /></label>
                    <label class="muted">dy <input type="number" value={step.dy} onchange={(e) => (step.dy = int(e.currentTarget.value))} /></label>
                  {:else}
                    <select bind:value={step.button}>
                      {#each mouseButtons as b (b)}<option value={b}>{b}</option>{/each}
                    </select>
                    {#if step.type === "click"}
                      <label class="muted">hold <input type="number" min="0" max="60000" placeholder="50" value={holdValue(step.holdMs)} onchange={(e) => (step.holdMs = parseHold(e.currentTarget.value))} /> ms</label>
                    {/if}
                  {/if}
                  <span class="spacer"></span>
                  <button class="small" onclick={() => move(action.steps, si, -1)}>↑</button>
                  <button class="small" onclick={() => move(action.steps, si, 1)}>↓</button>
                  <button class="small" onclick={() => action.steps.splice(si, 1)}>×</button>
                </div>
              {/each}
              <button class="small" onclick={() => action.steps.push({ type: "click", button: "left", holdMs: null })}>Add step</button>
            {:else}
              <SoundpadEditor bind:command={action.command} />
            {/if}
          </div>
        {/if}
      </div>
    {:else}
      <span class="muted small">none</span>
    {/each}
    <div class="row">
      <button class="small" onclick={() => addAction("keyboardMacro")}>+ Keyboard macro</button>
      <button class="small" onclick={() => addAction("mouseMacro")}>+ Mouse macro</button>
      <button class="small" onclick={() => addAction("soundpad")}>+ Soundpad</button>
    </div>
  </div>
</div>

<style>
  .actions { display: grid; grid-template-columns: 9em 1fr; gap: 4px 8px; align-items: start; padding: 4px 0; }
  .label { color: var(--muted); font-size: 12px; padding-top: 3px; }
  .body { display: flex; flex-direction: column; gap: 6px; }
  .action { border: 1px solid var(--line); border-radius: 4px; padding: 4px 6px; }
  .head { flex-wrap: nowrap; }
  .link { border: none; background: none; padding: 0; text-align: left; font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .spacer { flex: 1; }
  .steps { display: flex; flex-direction: column; gap: 4px; margin: 6px 0 2px 12px; align-items: flex-start; }
  .step { font-size: 12px; width: 100%; }
  .n { width: 1.5em; text-align: right; }
  .key { width: 10em; }
  .bad { border-color: var(--bad); }
  .small { font-size: 12px; }
  label { display: inline-flex; gap: 4px; align-items: center; }
</style>
