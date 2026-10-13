<script lang="ts">
  // Settings > General: tray, startup and administrator rights. Changes are saved at once.
  import type { AppSettings, AutostartKind, EngineSnapshot } from "../../lib/types";
  import { backend, errorText } from "../../lib/backend";

  let { snapshot }: { snapshot: EngineSnapshot } = $props();

  let settings = $derived(snapshot.settings);
  let shell = $derived(snapshot.shell);
  let error = $state("");
  let restarting = $state(false);

  const AUTOSTART: Record<AutostartKind, string> = {
    off: "not registered",
    runKey: "registered as a Windows startup entry (without administrator rights)",
    task: "registered as a Task Scheduler task (as administrator, no prompt)",
  };

  async function change(patch: Partial<AppSettings>) {
    error = "";
    try {
      await (await backend()).setSettings({ ...settings, ...patch });
    } catch (e) {
      error = errorText(e);
    }
  }

  async function restart() {
    error = "";
    restarting = true;
    try {
      await (await backend()).restartElevated();
    } catch (e) {
      error = errorText(e);
    } finally {
      restarting = false;
    }
  }
</script>

<section class="panel">
  <h2>General</h2>

  <div class="group">
    <h3>Window and tray</h3>
    <label class="check">
      <input type="checkbox" checked={settings.closeToTray} onchange={(e) => change({ closeToTray: e.currentTarget.checked })} />
      Close button minimizes to the tray
    </label>
    <p class="hint muted">
      Inputs, actions and the overlay keep running while the window is closed. Open it again with the tray icon; quit from
      the tray icon's menu.
    </p>
    <label class="check">
      <input type="checkbox" checked={settings.startMinimized} onchange={(e) => change({ startMinimized: e.currentTarget.checked })} />
      Start minimized to the tray
    </label>
    <p class="hint muted">Also when you start Trimcraft yourself. A start with Windows always goes to the tray.</p>
  </div>

  <div class="group">
    <h3>Startup</h3>
    <label class="check">
      <input type="checkbox" checked={settings.startWithWindows} onchange={(e) => change({ startWithWindows: e.currentTarget.checked })} />
      Start with Windows
    </label>
    <p class="hint muted">Autostart: {AUTOSTART[shell.autostart]}.</p>
    {#if shell.autostartNote}<p class="hint warn">{shell.autostartNote}</p>{/if}
  </div>

  <div class="group">
    <h3>Administrator rights</h3>
    <p class="state">
      Trimcraft runs as administrator:
      <b class:yes={shell.elevated}>{shell.elevated ? "yes" : "no"}</b>
    </p>
    <label class="check">
      <input type="checkbox" checked={settings.runAsAdmin} onchange={(e) => change({ runAsAdmin: e.currentTarget.checked })} />
      Always run as administrator
    </label>
    <p class="hint muted">
      Windows does not let Trimcraft's keys and mouse reach a program that runs as administrator (some games and anti-cheat
      launchers do) unless Trimcraft runs as administrator too. Trimcraft then asks for permission (UAC) when it starts; with
      Start with Windows it starts through the Task Scheduler without asking.
    </p>
    {#if !shell.elevated}
      <div class="row">
        <button onclick={restart} disabled={restarting}>{restarting ? "Waiting for Windows…" : "Restart as administrator"}</button>
      </div>
    {/if}
  </div>

  <div class="group">
    <h3>Macros</h3>
    <label class="check">Panic hotkey
      <input
        class="mono"
        value={settings.panicHotkey}
        placeholder="Ctrl+Shift+F12"
        onchange={(e) => change({ panicHotkey: e.currentTarget.value.trim() || "Ctrl+Shift+F12" })}
      />
    </label>
    <p class="hint muted">
      Stops every running macro and releases all keys and mouse buttons macros still hold, also while a game has the
      focus. Modifiers and one key, e.g. Ctrl+Shift+F12.
    </p>
  </div>

  {#if error}<p class="error small">{error}</p>{/if}
</section>

<style>
  .group { display: flex; flex-direction: column; gap: 4px; padding: 8px 0; border-top: 1px solid var(--line); }
  .group:first-of-type { border-top: none; padding-top: 0; }
  .check { display: inline-flex; gap: 8px; align-items: center; cursor: pointer; }
  .hint { margin: 0 0 4px 24px; font-size: 12px; }
  .hint.warn { color: var(--warn); }
  .state { margin: 0 0 4px; }
  b.yes { color: var(--ok); }
  .small { font-size: 12px; }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
