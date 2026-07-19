<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { SvelteSet } from "svelte/reactivity";
  import { app, recycleItems, refresh } from "./store.svelte";

  const selNames = new SvelteSet<string>();

  function rowClick(name: string, ev: MouseEvent) {
    if (ev.ctrlKey) {
      if (selNames.has(name)) selNames.delete(name);
      else selNames.add(name);
    } else {
      selNames.clear();
      selNames.add(name);
    }
  }

  async function restore() {
    if (!selNames.size) return;
    try {
      await invoke("recycle_restore", { names: [...selNames] });
      selNames.clear();
      refresh();
    } catch (e) {
      app.status = String(e);
    }
  }

  async function emptyBin() {
    try {
      // Windows shows its own confirmation dialog before deleting anything.
      await invoke("recycle_empty");
      refresh();
    } catch {
      refresh(); // cancelled or failed: just re-list
    }
  }
</script>

<div class="recyclepane">
  <div class="actions">
    <button class="abtn" disabled={!selNames.size} onclick={restore}>
      <span class="glyph">&#xE777;</span> Restore selected
    </button>
    <button class="abtn" disabled={!recycleItems.list.length} onclick={emptyBin}>
      <span class="glyph">&#xE74D;</span> Empty Recycle Bin
    </button>
  </div>

  {#if !recycleItems.list.length}
    <div class="empty">Recycle Bin is empty.</div>
  {:else}
    <div class="rheader">
      <span class="c name">Name</span>
      <span class="c">Original location</span>
      <span class="c">Date deleted</span>
      <span class="c size">Size</span>
    </div>
    <div class="rows">
      {#each recycleItems.list as it (it.name + it.deleted)}
        <button
          class="rrow"
          class:selected={selNames.has(it.name)}
          onclick={(ev) => rowClick(it.name, ev)}
          ondblclick={restore}
          title={it.original}
        >
          <span class="c name">{it.name}</span>
          <span class="c dim">{it.original}</span>
          <span class="c dim">{it.deleted}</span>
          <span class="c size dim">{it.size}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .recyclepane {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 8px 12px;
  }
  .actions {
    display: flex;
    gap: 8px;
    padding: 4px 4px 10px;
    flex-shrink: 0;
  }
  .abtn {
    height: 32px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    font-size: 13px;
    border: 1px solid var(--stroke-strong);
    border-radius: 4px;
  }
  .abtn:hover:not(:disabled) {
    background: var(--hover);
  }
  .abtn .glyph {
    font-size: 14px;
    color: var(--text-2);
  }
  .empty {
    flex: 1;
    display: grid;
    place-items: center;
    font-size: 13px;
    color: var(--text-3);
  }
  .rheader,
  .rrow {
    display: grid;
    grid-template-columns: minmax(160px, 1fr) minmax(200px, 1.4fr) 150px 90px;
    gap: 8px;
    align-items: center;
    padding: 0 10px;
  }
  .rheader {
    height: 34px;
    font-size: 13px;
    color: var(--text-2);
    border-bottom: 1px solid var(--stroke);
    flex-shrink: 0;
  }
  .rows {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .rrow {
    width: 100%;
    height: 34px;
    font-size: 13px;
    text-align: left;
    border-radius: 4px;
  }
  .rrow:hover {
    background: var(--hover);
  }
  .rrow.selected {
    background: var(--sel);
  }
  .c {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .c.size {
    text-align: right;
  }
  .dim {
    color: var(--text-3);
    font-size: 12px;
  }
</style>
