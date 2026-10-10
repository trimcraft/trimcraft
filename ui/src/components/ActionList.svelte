<script lang="ts">
  // Edits a list of actions: keyboard and mouse macros (see MacroEditor) and Soundpad commands.
  import type { Action, EngineSnapshot, KeyInfo, KeyboardMacro, MouseMacro, Playback } from "../lib/types";
  import { actionText } from "../lib/format";
  import MacroEditor from "./MacroEditor.svelte";
  import SoundpadEditor from "./SoundpadEditor.svelte";

  let {
    label,
    actions = $bindable(),
    keys,
    snapshot,
  }: { label: string; actions: Action[]; keys: KeyInfo[]; snapshot: EngineSnapshot } = $props();

  let open = $state<number | null>(null);

  const playback = (): Playback => ({ speedPercent: 100, minDelayMs: 0, endpointOnly: false });

  function newAction(type: Action["type"]): Action {
    switch (type) {
      case "keyboardMacro":
        return { type, steps: [{ type: "keyTap", key: "F1", holdMs: null }], playback: playback() };
      case "mouseMacro":
        return { type, steps: [{ type: "click", button: "left", holdMs: null }], playback: playback(), movement: "absolute", monitor: null };
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
    open = open === i ? j : open === j ? i : open;
  }
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
            {#if action.type === "soundpad"}
              <SoundpadEditor bind:command={action.command} />
            {:else}
              <MacroEditor bind:action={() => action as KeyboardMacro | MouseMacro, (v) => (actions[ai] = v)} {keys} {snapshot} />
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
  .body { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .action { border: 1px solid var(--line); border-radius: 4px; padding: 4px 6px; }
  .head { flex-wrap: nowrap; }
  .link { border: none; background: none; padding: 0; text-align: left; font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .spacer { flex: 1; }
  .steps { display: flex; flex-direction: column; gap: 4px; margin: 6px 0 2px 12px; align-items: flex-start; }
  .small { font-size: 12px; }
</style>
