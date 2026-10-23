<!-- Network: the computers Windows finds, and a way to reach one by name when
     it finds none (network discovery off, or a computer that doesn't announce
     itself). A computer opens as its shares; a share as a folder. -->
<script lang="ts">
  import { opener } from "./opener";
  import { msg } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { app, network, loadNetwork, navigate, openPath, loadDrives } from "./store.svelte";

  let typed = $state("\\\\");

  async function go(ev: SubmitEvent) {
    ev.preventDefault();
    const v = typed.trim();
    if (!v || v === "\\\\") return;
    try {
      const r = await invoke<{ kind: string; path: string }>("resolve_path", { input: v });
      if (r.kind === "file") await openPath(r.path);
      else await navigate(r.path);
    } catch (e) {
      app.status = String(e);
    }
  }

  async function mapDrive() {
    if (await invoke<boolean>("map_drive").catch((e) => ((app.status = String(e)), false))) await loadDrives();
  }
</script>

<div class="netpane">
  <form class="goto" onsubmit={go}>
    <label for="net-path">{msg("Go to a computer or shared folder")}</label>
    <div class="row">
      <input id="net-path" bind:value={typed} spellcheck="false" placeholder={msg("\\\\server\\share")} />
      <button class="btn" type="submit">{msg("Go")}</button>
      <button class="btn" type="button" onclick={mapDrive}>{msg("Map network drive...")}</button>
    </div>
  </form>
  <div class="section">
    {msg("Computers")}{network.loading ? "" : ` (${network.places.length})`}
    <button class="refresh" title={msg("Look again")} aria-label={msg("Look again")} disabled={network.loading} onclick={loadNetwork}>
      <span class="glyph" aria-hidden="true">&#xE72C;</span>
    </button>
  </div>
  {#if network.loading}
    <p class="note">{msg("Looking for computers on the network…")}</p>
  {:else if network.error}
    <p class="note">{msg("Couldn't look for computers on the network.")} {network.error}</p>
  {:else if !network.places.length}
    <p class="note">
      {msg("No computers were found. Network discovery may be off in Windows' network settings, or nothing here is shared. A computer can still be opened by its name above, like \\\\PC-NAME.")}
    </p>
  {/if}
  <div class="tiles">
    {#each network.places as p (p.path)}
      <button class="nettile" {...opener(() => navigate(p.path))} title={p.path}>
        <span class="glyph icon" aria-hidden="true">&#xE7F8;</span>
        <span class="name">{p.name}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .netpane {
    flex: 1;
    overflow-y: auto;
    padding: 12px 16px;
  }
  .goto {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0 4px 18px;
    font-size: 13px;
    color: var(--text-2);
  }
  .row {
    display: flex;
    gap: 8px;
    max-width: 640px;
  }
  .row input {
    flex: 1;
    min-width: 0;
    height: 32px;
    padding: 0 10px;
    background: var(--input-bg);
    border: 1px solid var(--stroke);
    border-bottom: 2px solid var(--accent);
    border-radius: 4px;
    color: var(--text);
    user-select: text;
  }
  .btn {
    height: 32px;
    padding: 0 12px;
    border: 1px solid var(--stroke-strong);
    border-radius: 4px;
    background: var(--input-bg);
    white-space: nowrap;
  }
  .btn:hover {
    background: var(--surface-hover);
  }
  .section {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--text-2);
    padding: 4px 0 10px 4px;
  }
  .refresh {
    width: 26px;
    height: 26px;
    border-radius: 4px;
    font-size: 12px;
    color: var(--text-2);
  }
  .refresh:hover:not(:disabled) {
    background: var(--hover);
  }
  .note {
    max-width: 640px;
    margin: 0 4px 12px;
    font-size: 13px;
    line-height: 1.45;
    color: var(--text-3);
  }
  .tiles {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .nettile {
    width: 220px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-radius: 4px;
    text-align: left;
  }
  .nettile:hover {
    background: var(--hover);
  }
  .nettile .icon {
    font-size: 32px;
    color: var(--text-2);
  }
  .name {
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
