<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app, sel, fmtSizeAuto, selIndices } from "./store.svelte";

  let selSize = $state(0);
  const selN = $derived(app.allSelected ? app.total : sel.size);

  $effect(() => {
    void sel.size;
    void app.allSelected;
    if (!selN) {
      selSize = 0;
      return;
    }
    invoke<[number, number]>("selection_stats", { indices: selIndices() }).then(
      ([, size]) => (selSize = size),
    );
  });
</script>

<div class="statusbar">
  <span class="stat">{app.total.toLocaleString()} items{app.searching ? " — searching…" : ""}</span>
  {#if selN}
    <span class="statsep"></span>
    <span class="stat">
      {selN.toLocaleString()} item{selN > 1 ? "s" : ""} selected
      {#if selSize}&nbsp;{fmtSizeAuto(selSize)}{/if}
    </span>
  {/if}
  {#if app.status}
    <span class="statsep"></span>
    <span class="stat error">{app.status}</span>
  {/if}
  <div class="fill"></div>
  <button class="viewtoggle" class:active={app.view === "details"} title="Details view" onclick={() => (app.view = "details")}>
    <span class="glyph">&#xE9D5;</span>
  </button>
  <button class="viewtoggle" class:active={app.view === "icons_l"} title="Large icons view" onclick={() => (app.view = "icons_l")}>
    <span class="glyph">&#xE8A9;</span>
  </button>
</div>

<style>
  .statusbar {
    height: 32px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    border-top: 1px solid var(--stroke);
    flex-shrink: 0;
  }
  .stat {
    font-size: 12px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .stat.error {
    color: #ff99a4;
  }
  .statsep {
    width: 1px;
    height: 14px;
    background: var(--stroke-strong);
  }
  .fill {
    flex: 1;
  }
  .viewtoggle {
    width: 32px;
    height: 24px;
    display: grid;
    place-items: center;
  }
  .viewtoggle .glyph {
    font-size: 14px;
    color: var(--text-3);
  }
  .viewtoggle.active .glyph {
    color: var(--accent);
  }
  .viewtoggle:hover:not(:disabled) {
    background: var(--hover);
  }
</style>
