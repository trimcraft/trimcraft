<script lang="ts">
  // Button that shows or hides THIRD_PARTY_LICENSES (Settings > About and the Credits page).
  import { backend, errorText } from "../../lib/backend";

  let text = $state<string | null>(null);
  let error = $state("");

  async function toggle() {
    error = "";
    if (text !== null) {
      text = null;
      return;
    }
    try {
      text = await (await backend()).thirdPartyLicenses();
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

<div class="row">
  <button onclick={toggle}>{text === null ? "Show third-party licences" : "Hide third-party licences"}</button>
</div>
{#if error}<p class="error">{error}</p>{/if}
{#if text !== null}
  <pre class="licenses">{text}</pre>
{/if}

<style>
  .row { display: flex; gap: 8px; flex-wrap: wrap; }
  .licenses {
    margin: 0;
    max-height: 420px;
    overflow: auto;
    padding: 8px;
    border: 1px solid var(--line);
    font-size: 11px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
