<script lang="ts">
  // Edits a "Launch" action: start a program, open a web page, or a file or folder.
  import type { LaunchAction, LaunchTarget } from "../../lib/types";
  import RunOptions from "./RunOptions.svelte";

  let { action = $bindable() }: { action: LaunchAction } = $props();

  function setType(type: LaunchTarget["type"]) {
    const t = action.target;
    if (type === "program") action.target = { type, path: t.type === "open" ? t.path : "", arguments: "", workingDir: "", onlyIfNotRunning: false };
    else if (type === "url") action.target = { type, url: t.type === "url" ? t.url : "https://" };
    else action.target = { type, path: t.type === "program" ? t.path : "" };
  }
</script>

<div class="editor">
  <div class="row">
    <select value={action.target.type} onchange={(e) => setType(e.currentTarget.value as LaunchTarget["type"])}>
      <option value="program">Start a program</option>
      <option value="url">Open a web page</option>
      <option value="open">Open a file or folder</option>
    </select>
  </div>
  {#if action.target.type === "program"}
    <label class="wide">Program <input class="mono grow" bind:value={action.target.path} placeholder="C:\Program Files\App\app.exe" /></label>
    <label class="wide">Arguments <input class="mono grow" bind:value={action.target.arguments} placeholder={'e.g. --profile "My Game"'} /></label>
    <label class="wide">Start in <input class="mono grow" bind:value={action.target.workingDir} placeholder="(the program's folder)" /></label>
    <label class="check"><input type="checkbox" bind:checked={action.target.onlyIfNotRunning} /> Only if it is not running already</label>
    <p class="muted small">Started directly, not through a command prompt: the arguments reach the program as written. When Detent runs as administrator, so does the program.</p>
  {:else if action.target.type === "url"}
    <label class="wide">Address <input class="mono grow" bind:value={action.target.url} placeholder="https://example.com" /></label>
    <p class="muted small">Opens in the default browser. Only http and https addresses.</p>
  {:else}
    <label class="wide">Path <input class="mono grow" bind:value={action.target.path} placeholder="C:\Users\me\Documents\checklist.pdf" /></label>
    <p class="muted small">Opens with the app Windows uses for it (like a double click).</p>
  {/if}
  <RunOptions {action} bind:wait={action.wait} />
</div>

<style>
  .editor { display: flex; flex-direction: column; gap: 6px; font-size: 12px; width: 100%; }
  label { display: inline-flex; gap: 4px; align-items: center; color: var(--muted); }
  .wide { display: flex; }
  .wide :global(.grow) { flex: 1; min-width: 20em; }
  .check { color: inherit; }
  .small { font-size: 11px; margin: 0; }
</style>
