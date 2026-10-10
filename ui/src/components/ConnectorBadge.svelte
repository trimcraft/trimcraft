<script lang="ts">
  // A connector's live status as a small pill.
  import type { ConnectorView } from "../lib/types";

  let { connector }: { connector: ConnectorView } = $props();

  let text = $derived.by(() => {
    switch (connector.status.state) {
      case "notConfigured":
        return "disabled";
      case "connecting":
        return "connecting…";
      case "connected":
        return "connected";
      case "error":
        return "not reachable";
    }
  });
</script>

<span
  class="badge {connector.status.state}"
  title={connector.status.state === "error" ? connector.status.message : `${connector.name}: ${text}`}
>● {text}</span>

<style>
  .connected { border-color: var(--ok); color: var(--ok); }
  .connecting { border-color: var(--warn); color: var(--warn); }
  .error { border-color: var(--bad); color: var(--bad); }
</style>
