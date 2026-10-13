<script lang="ts">
  // Connections: external programs Detent talks to. Each connector can be enabled, has its
  // own settings, a live status and a "Test connection" button. Its actions are offered in
  // the action editor only while it is enabled.
  import type { Connections, ConnectorId, ConnectorView, EngineSnapshot } from "../lib/types";
  import { backend, errorText } from "../lib/backend";
  import ConnectorBadge from "./ConnectorBadge.svelte";
  import DiscordSettings from "./DiscordSettings.svelte";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let error = $state("");
  /** Result of the last "Test connection", per connector. */
  let tests = $state<Partial<Record<ConnectorId, { ok: boolean; text: string; running: boolean }>>>({});

  async function save(change: (c: Connections) => void) {
    error = "";
    const next = $state.snapshot(snapshot.connections);
    change(next);
    try {
      await (await backend()).setConnections(next);
    } catch (e) {
      error = errorText(e);
    }
  }

  async function test(c: ConnectorView) {
    tests[c.id] = { ok: true, text: "testing…", running: true };
    try {
      tests[c.id] = { ok: true, text: await (await backend()).testConnection(c.id), running: false };
    } catch (e) {
      tests[c.id] = { ok: false, text: errorText(e), running: false };
    }
  }

  const int = (v: string) => Math.round(Number(v)) || 0;
</script>

<section class="panel">
  <h2>Connections</h2>
  <p class="muted small">
    Programs Detent can control. Enable a connection to use its actions in Virtual Controls; actions of a disabled
    connection stay saved but do not run.
  </p>
  {#if error}<p class="error">{error}</p>{/if}

  {#each snapshot.connectors as c (c.id)}
    <div class="connector" class:off={!c.enabled}>
      <div class="row head">
        <label class="toggle">
          <input
            type="checkbox"
            checked={c.enabled}
            onchange={(e) => {
              const on = e.currentTarget.checked;
              save((x) => (x[c.id].enabled = on));
            }}
          />
          <span class="name">{c.name}</span>
        </label>
        <ConnectorBadge connector={c} />
        {#if c.detail}<span class="muted small">{c.detail}</span>{/if}
        <span class="spacer"></span>
        <button class="small" disabled={tests[c.id]?.running} onclick={() => test(c)}>Test connection</button>
      </div>
      {#if tests[c.id]}
        <p class="small" class:error={!tests[c.id]!.ok} class:ok={tests[c.id]!.ok}>{tests[c.id]!.text}</p>
      {/if}
      {#if c.status.state === "error"}<p class="error small">{c.status.message}</p>{/if}
      {#if c.warning}<p class="warning small">Last action problem: {c.warning}</p>{/if}

      {#if c.id === "soundpad"}
        <p class="muted small">
          Plays sounds through Soundpad's remote control (named pipe <span class="mono">\\.\pipe\sp_remote_control</span>).
          Needs the full version of Soundpad running. While enabled, Detent reconnects on its own.
        </p>
        <div class="row settings">
          <label>Timeout per request
            <input type="number" min="100" max="5000" step="50" value={snapshot.connections.soundpad.timeoutMs}
              onchange={(e) => {
                const v = int(e.currentTarget.value);
                save((x) => (x.soundpad.timeoutMs = v));
              }} /> ms</label>
          <label title="A cached sound list older than this is fetched again before a sound is looked up">Refresh sound list after
            <input type="number" min="0" max="600" value={Math.round(snapshot.connections.soundpad.listMaxAgeMs / 1000)}
              onchange={(e) => {
                const v = int(e.currentTarget.value) * 1000;
                save((x) => (x.soundpad.listMaxAgeMs = v));
              }} /> s</label>
        </div>
      {:else if c.id === "discord"}
        <DiscordSettings connector={c} settings={snapshot.connections.discord} />
      {/if}
    </div>
  {/each}
</section>

<style>
  .connector { border-top: 1px solid var(--line); padding: 10px 0; display: flex; flex-direction: column; gap: 6px; }
  .toggle { display: inline-flex; gap: 8px; align-items: center; cursor: pointer; }
  .name { font-size: 16px; font-weight: 600; }
  .off .name { color: var(--muted); }
  .spacer { flex: 1; }
  .settings label { display: inline-flex; gap: 6px; align-items: center; color: var(--muted); font-size: 12px; }
  .small { font-size: 12px; margin: 0; }
  .ok { color: var(--ok); }
  .warning { color: var(--warn); }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
