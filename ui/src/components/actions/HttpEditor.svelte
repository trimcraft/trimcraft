<script lang="ts">
  // Edits an "HTTP request" action (webhook): method, URL, headers, body, timeout. A header
  // value can be stored as a secret: it goes straight to the Windows Credential Manager and
  // only a reference stays in the action.
  import type { HttpAction, HttpMethod } from "../../lib/types";
  import { backend, errorText } from "../../lib/backend";
  import { PLACEHOLDERS } from "../../lib/format";
  import RunOptions from "./RunOptions.svelte";

  let { action = $bindable() }: { action: HttpAction } = $props();

  const METHODS: HttpMethod[] = ["GET", "POST", "PUT", "PATCH", "DELETE"];

  /** Secret values being entered, by header row; never stored in the action. */
  let drafts = $state<Record<number, string>>({});
  /** Rows whose secret is being replaced. */
  let replacing = $state<Record<number, boolean>>({});
  let error = $state("");

  let hasBody = $derived(action.method !== "GET" && action.method !== "DELETE");

  function addHeader() {
    action.headers.push({ name: "", value: "", secret: null });
  }

  function removeHeader(i: number) {
    action.headers.splice(i, 1);
    drafts = {};
    replacing = {};
  }

  /** Plain value to secret: the typed value is stored right away. */
  function makeSecret(i: number) {
    const header = action.headers[i];
    if (!header) return;
    drafts[i] = header.value;
    header.value = "";
    replacing[i] = true;
    if (drafts[i]) storeSecret(i);
  }

  async function storeSecret(i: number) {
    const header = action.headers[i];
    const value = drafts[i] ?? "";
    if (!header || !value) return;
    error = "";
    try {
      header.secret = await (await backend()).storeHttpSecret(value);
      header.value = "";
      drafts[i] = "";
      replacing[i] = false;
    } catch (e) {
      error = errorText(e);
    }
  }

  function makePlain(i: number) {
    const header = action.headers[i];
    if (!header) return;
    header.secret = null;
    replacing[i] = false;
  }
</script>

<div class="editor">
  <div class="row">
    <select bind:value={action.method}>
      {#each METHODS as m (m)}<option value={m}>{m}</option>{/each}
    </select>
    <input class="mono grow" bind:value={action.url} placeholder="https://example.com/hook" />
  </div>
  <div class="headers">
    {#each action.headers as header, i (i)}
      <div class="row">
        <input class="mono name" bind:value={header.name} placeholder="Header, e.g. Authorization" />
        {#if header.secret && !replacing[i]}
          <span class="secret muted" title="Stored in the Windows Credential Manager ({header.secret.target})">•••••• stored as secret</span>
          <button class="small" onclick={() => (replacing[i] = true)}>Replace</button>
        {:else if header.secret || replacing[i]}
          <input class="mono value" type="password" bind:value={drafts[i]} placeholder="secret value" autocomplete="off" />
          <button class="small" disabled={!drafts[i]} onclick={() => storeSecret(i)}>Store as secret</button>
          {#if header.secret}<button class="small" onclick={() => (replacing[i] = false)}>Cancel</button>{/if}
        {:else}
          <input class="mono value" bind:value={header.value} placeholder="value" />
        {/if}
        <label class="check" title="Secret values are kept in the Windows Credential Manager, never in Trimcraft's settings or logs">
          <input type="checkbox" checked={!!header.secret || !!replacing[i]} onchange={(e) => (e.currentTarget.checked ? makeSecret(i) : makePlain(i))} /> secret
        </label>
        <button class="small" title="Remove header" onclick={() => removeHeader(i)}>×</button>
      </div>
    {/each}
    <div class="row"><button class="small" onclick={addHeader}>+ Header</button></div>
  </div>
  {#if hasBody}
    <textarea class="mono" rows="3" bind:value={action.body} placeholder={'{"text": "{control}: {state}"}'}></textarea>
  {/if}
  <p class="muted small">
    Placeholders in the address, header values and body: {PLACEHOLDERS.join(", ")}. In the address their values are URL-encoded.
    The result (status and duration) goes to the event log.
  </p>
  <div class="row">
    <label>Timeout <input type="number" min="0.1" max="60" step="0.5" value={action.timeoutMs / 1000} oninput={(e) => (action.timeoutMs = Math.round((e.currentTarget.valueAsNumber || 5) * 1000))} /> s</label>
  </div>
  {#if error}<p class="error small">{error}</p>{/if}
  <RunOptions {action} bind:wait={action.wait} />
</div>

<style>
  .editor { display: flex; flex-direction: column; gap: 6px; font-size: 12px; width: 100%; }
  .headers { display: flex; flex-direction: column; gap: 4px; }
  .grow { flex: 1; min-width: 20em; }
  .name { width: 14em; }
  .value { flex: 1; min-width: 12em; }
  .secret { flex: 1; }
  textarea { width: 100%; font: inherit; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; padding: 4px 6px; }
  label { display: inline-flex; gap: 4px; align-items: center; color: var(--muted); }
  .small { font-size: 11px; margin: 0; }
</style>
