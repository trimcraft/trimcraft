<script lang="ts">
  // Connections: external programs Detent talks to. Each connector can be enabled, has its
  // own settings, a live status and a "Test connection" button. Its actions are offered in
  // the action editor only while it is enabled.
  import type { Connections, ConnectorId, ConnectorView, EngineSnapshot } from "../lib/types";
  import { backend, errorText } from "../lib/backend";
  import ConnectorBadge from "./ConnectorBadge.svelte";
  import DiscordSettings from "./DiscordSettings.svelte";
  import { vjoyAxisName } from "../lib/format";

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
    Programs and drivers Detent can control. Enable a connection to use its actions in Virtual Controls; actions of a disabled
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
      {:else if c.id === "vjoy"}
        <p class="muted small">
          Feeds virtual joysticks (vJoy devices 1–16) that games see as controllers: state outputs of virtual controls,
          vJoy mappings and vJoy actions. While enabled, Detent owns the devices the active profile uses and gives them back
          when disabled or on exit. Axes, buttons and POVs of a device are set in <b>Configure vJoy</b> (vJoyConf, in the
          Start menu); Detent cannot change them. Check the output with <b>vJoy Monitor</b> or Windows' <span class="mono">joy.cpl</span>.
        </p>
        {#if c.vjoy?.version}
          <table class="devices">
            <thead><tr><th>Device</th><th>Axes</th><th>Buttons</th><th>POVs</th><th>Status</th></tr></thead>
            <tbody>
              {#each c.vjoy.devices.filter((d) => d.owner !== "missing") as d (d.id)}
                <tr>
                  <td>vJoy {d.id}</td>
                  <td>{d.axes.map(vjoyAxisName).join(", ") || "none"}</td>
                  <td class:warning={d.buttons === 0}>{d.buttons}</td>
                  <td>{d.continuousPovs ? `${d.continuousPovs} (8-way)` : d.discretePovs ? `${d.discretePovs} (4-way)` : "0"}</td>
                  <td class:ok={d.owner === "detent"} class:warning={d.owner === "other"}>
                    {d.owner === "detent" ? "owned by Detent" : d.owner === "other" ? "used by another program" : "free"}
                  </td>
                </tr>
              {:else}
                <tr><td colspan="5" class="warning">No vJoy device is configured: add one in Configure vJoy.</td></tr>
              {/each}
            </tbody>
          </table>
          {#if c.vjoy.devices.some((d) => d.owner !== "missing" && d.buttons === 0)}
            <p class="warning small">A device without buttons cannot take button outputs: add buttons (and POVs) in Configure vJoy.</p>
          {/if}
        {/if}
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
  .devices { border-collapse: collapse; font-size: 12px; }
  .devices th { text-align: left; color: var(--muted); font-weight: normal; padding: 2px 12px 2px 0; }
  .devices td { padding: 2px 12px 2px 0; border-top: 1px solid var(--line); }
</style>
