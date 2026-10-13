<script lang="ts">
  // The Discord card on the Connections page: client ID and secret of the user's own Discord
  // application, the redirect URI to register, authorization and live mute/deaf state. The
  // secret goes straight to the credential store; it is never shown again or put in setup.json.
  import type { ConnectorView, DiscordSettings } from "../lib/types";
  import { backend, errorText } from "../lib/backend";
  import VoiceIndicator from "./VoiceIndicator.svelte";

  let { connector, settings }: { connector: ConnectorView; settings: DiscordSettings } = $props();

  const REDIRECT_URI = "https://localhost/";

  // Edited until saved; follows the saved value while untouched.
  let clientId = $state("");
  let editedId = $state(false);
  $effect(() => {
    if (!editedId) clientId = settings.clientId;
  });
  let secret = $state("");
  let showSecret = $state(false);
  let busy = $state(false);
  let message = $state<{ ok: boolean; text: string } | null>(null);
  let confirmRemove = $state(false);
  let copied = $state(false);

  let creds = $derived(connector.credentials ?? { secretSaved: false, authorized: false });

  async function run(f: () => Promise<void>, done: string) {
    busy = true;
    message = null;
    try {
      await f();
      message = { ok: true, text: done };
    } catch (e) {
      message = { ok: false, text: errorText(e) };
    } finally {
      busy = false;
    }
  }

  function save() {
    return run(async () => {
      await (await backend()).saveDiscordCredentials(clientId.trim(), secret.trim() || null);
      secret = "";
      showSecret = false;
      editedId = false;
    }, "Saved. The secret is in the Windows Credential Manager.");
  }

  function forget() {
    return run(async () => (await backend()).forgetDiscordAuthorization(), "Authorization forgotten. Test connection asks in Discord again.");
  }

  function remove() {
    if (!confirmRemove) {
      confirmRemove = true;
      setTimeout(() => (confirmRemove = false), 4000);
      return;
    }
    confirmRemove = false;
    return run(async () => {
      await (await backend()).removeDiscordCredentials();
      secret = "";
      editedId = false;
    }, "Client ID, secret and authorization removed.");
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(REDIRECT_URI);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      message = { ok: false, text: `cannot copy: ${errorText(e)}` };
    }
  }
</script>

<p class="muted small">
  Mutes and deafens you in Discord through its local RPC interface (Discord must be running). Uses your own application
  from the <span class="mono">Discord Developer Portal</span>: enter its Client ID and Client Secret, save, then press
  Test connection and approve the request in Discord. While enabled, Detent stays connected and reconnects on its own.
</p>

<div class="redirect row">
  <span class="muted small">Redirect URI to add under OAuth2, Redirects:</span>
  <span class="mono">{REDIRECT_URI}</span>
  <button class="small" onclick={copy}>{copied ? "Copied" : "Copy"}</button>
</div>

<div class="fields">
  <label for="discord-client-id">Client ID</label>
  <input id="discord-client-id" class="mono" inputmode="numeric" autocomplete="off" spellcheck="false" placeholder="e.g. 1234567890123456789"
    value={clientId}
    oninput={(e) => { clientId = e.currentTarget.value; editedId = true; }} />

  <label for="discord-client-secret">Client Secret</label>
  <div class="row">
    <input id="discord-client-secret" class="mono secret" type={showSecret ? "text" : "password"} autocomplete="off" spellcheck="false"
      placeholder={creds.secretSaved ? "saved (leave empty to keep)" : "from the OAuth2 page"}
      bind:value={secret} />
    <button class="small" onclick={() => (showSecret = !showSecret)} title={showSecret ? "Hide" : "Show"}>{showSecret ? "Hide" : "Show"}</button>
  </div>
</div>

<div class="row">
  <button class="primary" disabled={busy} onclick={save}>Save</button>
  <button class="small" disabled={busy || !creds.authorized} onclick={forget} title="Deletes the stored authorization (tokens) and disconnects">Disconnect / forget authorization</button>
  <button class="small" class:danger={confirmRemove} disabled={busy || (!creds.secretSaved && !settings.clientId)} onclick={remove}>
    {confirmRemove ? "Really remove?" : "Remove credentials"}
  </button>
</div>

<p class="muted small">
  Client secret: {creds.secretSaved ? "saved" : "not saved"} · Authorization: {creds.authorized ? "stored" : "none yet"}
</p>
{#if connector.status.state === "waitingForApproval"}
  <p class="approve">Approve the request in your Discord client.</p>
{/if}
{#if connector.voice}
  <div class="row"><span class="muted small">Live:</span><VoiceIndicator voice={connector.voice} /></div>
{/if}
{#if message}<p class="small" class:error={!message.ok} class:ok={message.ok}>{message.text}</p>{/if}

<style>
  .small { font-size: 12px; margin: 0; }
  .fields { display: grid; grid-template-columns: 8em minmax(0, 26em); gap: 6px 8px; align-items: center; }
  .fields label { color: var(--muted); font-size: 12px; }
  .secret { flex: 1; min-width: 0; }
  .approve { margin: 0; color: var(--warn); font-weight: 600; }
  .ok { color: var(--ok); }
  .danger { border-color: var(--bad); color: var(--bad); }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
