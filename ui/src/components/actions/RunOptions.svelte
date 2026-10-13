<script lang="ts">
  // "Wait until finished" for long-running actions, and a Test button that runs the action as
  // edited (placeholders get "Test play", "editor" and the active profile).
  import type { Action } from "../../lib/types";
  import { backend, errorText } from "../../lib/backend";

  let { action, wait = $bindable() }: { action: Action; wait?: boolean } = $props();

  let error = $state("");
  let sent = $state(false);

  async function test() {
    error = "";
    try {
      await (await backend()).testAction($state.snapshot(action));
      sent = true;
      setTimeout(() => (sent = false), 1500);
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

<div class="row">
  {#if wait !== undefined}
    <label class="check" title="Off: the action starts and the next one runs at once. On: the next action waits until this one has finished.">
      <input type="checkbox" bind:checked={wait} /> Wait until finished
    </label>
  {/if}
  <button class="small" onclick={test}>▶ Test</button>
  {#if sent}<span class="muted small">started, see the event log</span>{/if}
</div>
{#if error}<p class="error small">{error}</p>{/if}

<style>
  .check { display: inline-flex; gap: 4px; align-items: center; font-size: 12px; }
  .small { font-size: 12px; }
  p { margin: 0; }
</style>
