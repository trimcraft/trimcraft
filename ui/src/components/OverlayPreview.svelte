<script lang="ts">
  // The overlay as the window will draw it: the current appearance, the active profile's
  // widgets and a sample toast. Used in Settings and in the Overlay panel.
  import type { Appearance, EngineSnapshot } from "../lib/types";
  import { overlayStyle } from "../lib/overlay-style";
  import OverlayWidgets from "./OverlayWidgets.svelte";
  import ToastView from "./ToastView.svelte";

  let { snapshot, appearance = snapshot.overlay.appearance }: { snapshot: EngineSnapshot; appearance?: Appearance } = $props();

  const sample = [{ id: 1, key: "preview", title: "COM1", text: "121.500", version: 0 }];
</script>

<div class="preview" style={overlayStyle(appearance)}>
  {#if snapshot.overlay.widgets.length}
    <div class="panel"><OverlayWidgets {snapshot} /></div>
  {:else}
    <p class="hint">Add controls to the overlay to see them here.</p>
  {/if}
  <ToastView toasts={sample} />
</div>

<style>
  .preview {
    background: repeating-linear-gradient(45deg, #1c2128, #1c2128 10px, #20262e 10px, #20262e 20px);
    padding: 12px;
    border-radius: 6px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    align-items: flex-start;
    color: var(--text);
  }
  .panel { background: var(--ov-bg); border: 1px solid var(--ov-border); border-radius: var(--ov-radius); padding: 10px 14px; }
  .hint { margin: 0; font-size: 12px; color: #aab; }
</style>
