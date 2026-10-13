<script lang="ts">
  // The Discord card on the Connections page: a setup guide, client ID and secret of the
  // user's own Discord application, authorization and live mute/deaf state. The secret goes
  // straight to the credential store; it is never shown again or put in setup.json.
  //
  // The guide's redirect URI and scopes come from the backend (the constants the connector
  // uses), and errors point at the step that fixes them.
  import type { ConnectorView, DiscordSettings, DiscordSetup, GuideStep } from "../lib/types";
  import { backend, errorText } from "../lib/backend";
  import VoiceIndicator from "./VoiceIndicator.svelte";

  let { connector, settings }: { connector: ConnectorView; settings: DiscordSettings } = $props();

  let setup = $state<DiscordSetup | null>(null);
  $effect(() => {
    backend()
      .then((b) => b.discordSetup())
      .then((s) => (setup = s))
      .catch((e) => (message = { ok: false, text: errorText(e) }));
  });

  /** Guide step numbers (see the list below). */
  const STEP: Record<GuideStep, number> = {
    clientId: 3,
    clientSecret: 4,
    redirect: 5,
    discordRunning: 7,
    authorize: 8,
    appTesters: 9,
  };
  let hint = $derived(connector.guide ? STEP[connector.guide] : null);

  // Open while not connected (or when a step needs attention); the user can toggle it.
  let manualOpen = $state<boolean | null>(null);
  let lastGuide: GuideStep | null = null;
  $effect(() => {
    // A new hint opens the guide again (snapshots arrive often; only a change counts).
    const guide = connector.guide;
    if (guide !== lastGuide) {
      lastGuide = guide;
      if (guide) manualOpen = null;
    }
  });
  let guideOpen = $derived(manualOpen ?? (connector.status.state !== "connected" || hint !== null));

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
    }, "Saved. The secret is stored in the Windows Credential Manager. Next: step 7 and 8.");
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

  async function openPortal() {
    try {
      await (await backend()).openDiscordPortal();
    } catch (e) {
      message = { ok: false, text: errorText(e) };
    }
  }

  async function copyRedirect() {
    if (!setup) return;
    try {
      await navigator.clipboard.writeText(setup.redirectUri);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      message = { ok: false, text: `cannot copy: ${errorText(e)}` };
    }
  }
</script>

<p class="muted small">
  Mutes and deafens you in Discord through the Discord desktop app's local interface. Trimcraft uses a Discord application that
  you create yourself (free, takes about two minutes); its Client ID and Client Secret go into the fields below.
</p>

{#if hint !== null}
  <p class="hint">See step {hint} of the setup guide below.</p>
{/if}

<div class="guide">
  <button class="link toggle" onclick={() => (manualOpen = !guideOpen)} aria-expanded={guideOpen}>
    {guideOpen ? "▾" : "▸"} How to set up
  </button>
  {#if guideOpen}
    <ol>
      <li>
        Open the <button class="link" onclick={openPortal}>Discord Developer Portal</button> in your browser and log in with
        your Discord account.
      </li>
      <li>
        Click <b>New Application</b> (top right). Enter a name, e.g. <i>Trimcraft</i>, tick the box to accept the Developer
        Terms of Service and Developer Policy, and click <b>Create</b>. Your application's <i>General Information</i> page
        opens.
      </li>
      <li class:marked={hint === 3}>
        In the left sidebar, click <b>OAuth2</b>. Under <i>Client information</i>, copy the <b>Client ID</b> (a long number)
        and paste it into <b>Client ID</b> below. It tells Discord which application is asking.
      </li>
      <li class:marked={hint === 4}>
        On the same page, under <i>Client Secret</i>, click <b>Reset Secret</b> and confirm. Copy the secret and paste it into
        <b>Client Secret</b> below. Discord shows it only once; if you lose it, reset it again (and paste the new one here).
        Trimcraft needs it to turn your approval into a login; it is stored in the Windows Credential Manager, not in a file.
      </li>
      <li class:marked={hint === 5}>
        Still on the OAuth2 page, under <i>Redirects</i>, click <b>Add Redirect</b> and enter
        <span class="mono">{setup?.redirectUri ?? "…"}</span>
        <button class="small" onclick={copyRedirect} disabled={!setup}>{copied ? "Copied" : "Copy"}</button>
        exactly as shown. Then click <b>Save Changes</b> in the bar that appears at the bottom. Nothing runs at this address;
        Discord only requires the value to match.
      </li>
      <li>
        On the same page, leave <b>Public Client</b> switched off.
      </li>
      <li class:marked={hint === 7}>
        Make sure the Discord <b>desktop app</b> is running and you are logged in (the browser version cannot be controlled).
        Close other tools that control Discord's mute and deafen, e.g. a Stream Deck Discord plugin: Discord lets only one
        app change voice settings at a time.
      </li>
      <li class:marked={hint === 8}>
        Here in Trimcraft: click <b>Save</b> below, tick the box next to <b>Discord</b> at the top of this card to enable it, and
        click <b>Test connection</b>. Discord shows a window asking whether your application may access your account; click
        <b>Authorize</b>. Success: the status turns <span class="ok">● connected</span> and the test shows
        <i>Connected as &lt;your name&gt;, mic muted: …, deafened: …</i>. You approve only once; Trimcraft reconnects on its
        own after restarts.
      </li>
      <li class:marked={hint === 9}>
        <b>Troubleshooting:</b> if Discord refuses access (the error mentions App Testers), open your application in the
        Developer Portal, click <b>App Testers</b> in the left sidebar, add your own Discord account, accept the invitation
        Discord sends you (by e-mail), then click <b>Test connection</b> again.
      </li>
    </ol>
    {#if setup}
      <p class="muted small">Trimcraft asks Discord for: <span class="mono">{setup.scopes.join(", ")}</span> (read and change your mute and deafen state).</p>
    {/if}
  {/if}
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
  <p class="approve">Approve the request in your Discord client (click Authorize).</p>
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
  .hint { margin: 0; font-size: 12px; color: var(--warn); }
  .ok { color: var(--ok); }
  .danger { border-color: var(--bad); color: var(--bad); }
  .guide { border: 1px solid var(--line); border-radius: 4px; padding: 6px 10px; }
  .guide ol { margin: 6px 0; padding-left: 22px; font-size: 12px; line-height: 1.5; }
  .guide li { padding: 2px 4px; border-radius: 3px; }
  .guide li.marked { background: var(--accent-dim); outline: 1px solid var(--warn); }
  .link { border: none; background: none; padding: 0; color: var(--accent); text-decoration: underline; cursor: pointer; font-size: inherit; }
  .toggle { text-decoration: none; font-weight: 600; color: var(--text); }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
