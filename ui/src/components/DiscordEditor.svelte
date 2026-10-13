<script lang="ts">
  // Edits one Discord action: mute, unmute, deafen, undeafen or a toggle. Shows the live
  // state, so it is clear what a toggle will do.
  import type { ConnectorView, DiscordCommand } from "../lib/types";
  import { DISCORD_COMMANDS } from "../lib/format";
  import ConnectorBadge from "./ConnectorBadge.svelte";
  import VoiceIndicator from "./VoiceIndicator.svelte";

  let { command = $bindable(), connector }: { command: DiscordCommand; connector: ConnectorView | undefined } = $props();
</script>

<div class="row">
  <select bind:value={command}>
    {#each DISCORD_COMMANDS as [value, label] (value)}
      <option {value}>{label}</option>
    {/each}
  </select>
  {#if connector}
    <ConnectorBadge {connector} />
    {#if connector.voice}<VoiceIndicator voice={connector.voice} />{/if}
  {/if}
</div>
<p class="muted small">
  Mute and deafen set the state and do nothing when it already holds; the toggles go by Discord's live state.
</p>

<style>
  .small { font-size: 11px; margin: 0; }
</style>
