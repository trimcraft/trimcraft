<script lang="ts">
  // Edits a list of actions: keyboard and mouse macros (see MacroEditor), Soundpad, Discord and
  // Voicemeeter commands, profile switches, speech, sound files, volume, media keys, launch and
  // HTTP requests (editors in ./actions).
  import type { Action, ConnectorId, EngineSnapshot, KeyInfo, KeyboardMacro, MouseMacro, Playback } from "../lib/types";
  import { show } from "../lib/nav.svelte";
  import { actionText } from "../lib/format";
  import MacroEditor from "./MacroEditor.svelte";
  import SoundpadEditor from "./SoundpadEditor.svelte";
  import DiscordEditor from "./DiscordEditor.svelte";
  import ProfileTargetEditor from "./ProfileTargetEditor.svelte";
  import SpeakEditor from "./actions/SpeakEditor.svelte";
  import PlaySoundEditor from "./actions/PlaySoundEditor.svelte";
  import VolumeEditor from "./actions/VolumeEditor.svelte";
  import VoicemeeterEditor from "./actions/VoicemeeterEditor.svelte";
  import LaunchEditor from "./actions/LaunchEditor.svelte";
  import HttpEditor from "./actions/HttpEditor.svelte";
  import { MEDIA_KEYS } from "../lib/format";

  let {
    label,
    actions = $bindable(),
    keys,
    snapshot,
  }: { label: string; actions: Action[]; keys: KeyInfo[]; snapshot: EngineSnapshot } = $props();

  let open = $state<number | null>(null);
  /** The connector an action needs (keyboard and mouse macros are built in). */
  const connectorOf = (a: Action): ConnectorId | null =>
    a.type === "soundpad" || a.type === "discord" || a.type === "voicemeeter" ? a.type : null;
  const connectorView = (id: ConnectorId) => snapshot.connectors.find((c) => c.id === id);
  /** A connector's actions are offered, and run, only while it is enabled (and installed). */
  const enabled = (id: ConnectorId) => snapshot.connections[id].enabled && !connectorView(id)?.missing;

  /** What "Add action" offers, in groups. */
  let addable = $derived.by(() => {
    const groups: [string, [Action["type"], string][]][] = [
      ["Input", [["keyboardMacro", "Keyboard macro"], ["mouseMacro", "Mouse macro"], ["mediaKey", "Media key"]]],
      ["Sound", [["speak", "Speak text"], ["playSound", "Play sound file"], ["volume", "Audio volume"]]],
      ["Programs", [["launch", "Launch program / open"], ["http", "HTTP request (webhook)"], ["switchProfile", "Switch profile"]]],
    ];
    const connectors: [Action["type"], string][] = [];
    if (enabled("soundpad")) connectors.push(["soundpad", "Soundpad"]);
    if (enabled("discord")) connectors.push(["discord", "Discord"]);
    if (enabled("voicemeeter")) connectors.push(["voicemeeter", "Voicemeeter"]);
    if (connectors.length) groups.push(["Connections", connectors]);
    return groups;
  });
  const inactive = (a: Action) => {
    const id = connectorOf(a);
    return id !== null && !enabled(id);
  };

  const playback = (): Playback => ({ speedPercent: 100, minDelayMs: 0, endpointOnly: false });

  function newAction(type: Action["type"]): Action {
    switch (type) {
      case "keyboardMacro":
        return { type, steps: [{ type: "keyTap", key: "F1", holdMs: null }], playback: playback() };
      case "mouseMacro":
        return { type, steps: [{ type: "click", button: "left", holdMs: null }], playback: playback(), movement: "absolute", monitor: null };
      case "soundpad":
        return { type, command: { type: "play", sound: { title: "", path: "" }, output: "both" } };
      case "discord":
        return { type, command: "toggleMute" };
      case "switchProfile":
        return { type, target: { type: "next" } };
      case "speak":
        return { type, text: "{state}", voice: null, rate: 0, volume: 100, wait: false };
      case "playSound":
        return { type, path: "", volume: 100, device: null, stopPrevious: false, wait: false };
      case "volume":
        return { type, target: { type: "master" }, command: { type: "toggleMute" } };
      case "voicemeeter":
        return { type, target: { kind: "strip", index: 0 }, command: { type: "toggleMute" } };
      case "mediaKey":
        return { type, key: "playPause" };
      case "launch":
        return { type, target: { type: "program", path: "", arguments: "", workingDir: "", onlyIfNotRunning: true }, wait: false };
      case "http":
        return { type, method: "POST", url: "https://", headers: [], body: "", timeoutMs: 5000, wait: false };
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
      <div class="action" class:inactive={inactive(action)}>
        <div class="row head">
          <button class="link" onclick={() => (open = open === ai ? null : ai)}>{open === ai ? "▾" : "▸"} {actionText(action, snapshot.profiles)}</button>
          <span class="spacer"></span>
          <button class="small" title="Move up" onclick={() => move(actions, ai, -1)}>↑</button>
          <button class="small" title="Move down" onclick={() => move(actions, ai, 1)}>↓</button>
          <button class="small" onclick={() => { actions.splice(ai, 1); open = null; }}>Delete</button>
        </div>
        {#if inactive(action)}
          <p class="hint">
            The {connectorView(connectorOf(action)!)?.name} connection is disabled: this action is kept but does not run.
            <button class="link small" onclick={() => show("connections")}>Open Connections</button>
          </p>
        {/if}
        {#if open === ai}
          <div class="steps">
            {#if action.type === "soundpad"}
              <SoundpadEditor bind:command={action.command} connector={connectorView("soundpad")} />
            {:else if action.type === "discord"}
              <DiscordEditor bind:command={action.command} connector={connectorView("discord")} />
            {:else if action.type === "switchProfile"}
              <ProfileTargetEditor bind:target={action.target} profiles={snapshot.profiles} />
            {:else if action.type === "speak"}
              <SpeakEditor bind:action={actions[ai] as typeof action} />
            {:else if action.type === "playSound"}
              <PlaySoundEditor bind:action={actions[ai] as typeof action} />
            {:else if action.type === "volume"}
              <VolumeEditor bind:target={action.target} bind:command={action.command} />
            {:else if action.type === "voicemeeter"}
              <VoicemeeterEditor bind:target={action.target} bind:command={action.command} connector={connectorView("voicemeeter")} />
            {:else if action.type === "mediaKey"}
              <div class="row">
                <select bind:value={action.key}>
                  {#each MEDIA_KEYS as [value, text] (value)}<option {value}>{text}</option>{/each}
                </select>
                <span class="muted small">Sent like a keyboard's media key: the active media player or Windows reacts.</span>
              </div>
            {:else if action.type === "launch"}
              <LaunchEditor bind:action={actions[ai] as typeof action} />
            {:else if action.type === "http"}
              <HttpEditor bind:action={actions[ai] as typeof action} />
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
      <select
        class="small add"
        value=""
        onchange={(e) => {
          const type = e.currentTarget.value as Action["type"];
          e.currentTarget.value = "";
          if (type) addAction(type);
        }}
      >
        <option value="" disabled>+ Add action…</option>
        {#each addable as [group, types] (group)}
          <optgroup label={group}>
            {#each types as [value, text] (value)}<option {value}>{text}</option>{/each}
          </optgroup>
        {/each}
      </select>
    </div>
  </div>
</div>

<style>
  .actions { display: grid; grid-template-columns: 9em 1fr; gap: 4px 8px; align-items: start; padding: 4px 0; }
  .label { color: var(--muted); font-size: 12px; padding-top: 3px; }
  .body { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .action { border: 1px solid var(--line); border-radius: 4px; padding: 4px 6px; }
  .action.inactive { opacity: 0.55; border-style: dashed; }
  .hint { margin: 2px 0 0; font-size: 11px; color: var(--warn); }
  .hint .link { color: var(--accent); text-decoration: underline; }
  .head { flex-wrap: nowrap; }
  .link { border: none; background: none; padding: 0; text-align: left; font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .spacer { flex: 1; }
  .steps { display: flex; flex-direction: column; gap: 4px; margin: 6px 0 2px 12px; align-items: flex-start; }
  .small { font-size: 12px; }
</style>
