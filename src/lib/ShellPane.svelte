<script lang="ts">
  import { msg } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { app, shellFolder, shellSelection, hooks, navigate, refresh } from "./store.svelte";
  import { DEVICES, shellLocation, type ShellEntry } from "./shellplaces";
  import { recordExternalCopy, type OpResult } from "./ops.svelte";
  let busy = $state(false);
  let scrollTop = $state(0);
  let height = $state(600);
  let headerHeight = $state(150);
  let pane: HTMLElement | undefined = $state();
  const start = $derived(Math.max(0, Math.floor((scrollTop - headerHeight - 20) / 42) - 6));
  const end = $derived(Math.min(shellFolder.items.length, start + Math.ceil(height / 42) + 14));
  const visible = $derived(shellFolder.items.slice(start, end));
  $effect(() => { void app.path; scrollTop = 0; if (pane) pane.scrollTop = 0; });
  hooks.getScroll = () => pane?.scrollTop ?? 0;
  hooks.scrollTo = (top) => { if (pane) pane.scrollTop = top; };
  async function open(item: ShellEntry) {
    if (item.folder) await navigate(shellLocation(item.path));
    else app.status = msg("Copy this file to a local folder to open it.");
  }
  async function copy() {
    busy = true;
    try {
      const paths = [...shellSelection.paths];
      const result = await invoke<OpResult | null>("shell_copy_out", { paths });
      if (result) recordExternalCopy(result);
    } catch (error) { app.status = String(error); }
    finally { busy = false; }
  }
</script>

<section class="shellpane" aria-label={msg("Devices and shell folders")} bind:this={pane} bind:clientHeight={height} onscroll={(event) => scrollTop = event.currentTarget.scrollTop}>
  <div class="heading" bind:clientHeight={headerHeight}>
  <div class="tools">
    <h1>{app.path === DEVICES ? msg("Devices") : shellFolder.name || msg("Device folder")}</h1>
    <button class="sbtn" onclick={copy} disabled={busy || !shellSelection.paths.length}>{msg("Copy to folder…")}</button>
    <button class="sbtn" onclick={refresh} disabled={busy}>{msg("Refresh")}</button>
  </div>
  <p>{msg("Connect and unlock your device to browse it. Filezy supports browsing and copying to a local folder here.")}</p>
  <p>{msg("Rename, delete, paste, preview and search are unavailable in this device view.")}</p>
  </div>
  {#if shellFolder.loading}<p role="status">{msg("Loading…")}</p>
  {:else if shellFolder.error}<p role="alert">{shellFolder.error}</p><button class="sbtn" onclick={refresh}>{msg("Try again")}</button>
  {:else}
    <div class="items" role="list" aria-label={msg("Device items")}>
      <div style={`height:${start * 42}px`} aria-hidden="true"></div>
      {#each visible as item, offset (item.path)}
        <div class="item" role="listitem" aria-setsize={shellFolder.items.length} aria-posinset={start + offset + 1}>
          <input type="checkbox" aria-label={msg("Select {name}", { name: item.name })} value={item.path} bind:group={shellSelection.paths} disabled={!item.copy || busy} title={!item.copy ? msg("The device does not support copying this item.") : undefined} />
          <button class="entry" onclick={() => open(item)} title={item.name}><span class="glyph" aria-hidden="true">{item.folder ? "\uE8B7" : "\uE7C3"}</span><span>{item.name}</span></button>
        </div>
      {:else}<p>{msg("No items. Check that your device is connected, unlocked and permits file transfer.")}</p>{/each}
      <div style={`height:${(shellFolder.items.length - end) * 42}px`} aria-hidden="true"></div>
    </div>
  {/if}
  {#if shellFolder.capped}<p role="status">{msg("Only the first 10,000 items are shown. Open a smaller folder to see more.")}</p>{/if}
</section>

<style>
  .shellpane { flex: 1; overflow: auto; padding: 20px; }
  .heading { display: flow-root; }
  .tools { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  h1 { font-size: 22px; flex: 1; overflow-wrap: anywhere; }
  p { margin: 12px 0; color: var(--text-2); }
  .item { display: flex; gap: 12px; align-items: center; height: 42px; }
  .entry { display: flex; align-items: center; gap: 12px; flex: 1; min-width: 0; padding: 10px; text-align: left; }
  .entry:hover { background: var(--hover); }
  .entry span:last-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
