<script lang="ts">
  // Discord's live mute and deaf state as two small pills.
  import type { VoiceState } from "../lib/types";

  let { voice }: { voice: VoiceState } = $props();
</script>

<span class="badge" class:on={voice.mute} title="Microphone in Discord">{voice.mute ? "🎙 muted" : "🎙 live"}</span>
<span class="badge" class:on={voice.deaf} title="Sound in Discord">{voice.deaf ? "🎧 deafened" : "🎧 hearing"}</span>

<style>
  .on { border-color: var(--warn); color: var(--warn); }
</style>
