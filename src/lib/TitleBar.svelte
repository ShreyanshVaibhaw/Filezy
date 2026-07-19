<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    app, tabs, extMeta, folderName, newTab, switchTab, closeTab,
    duplicateTab, closeOtherTabs, closeTabsRight,
  } from "./store.svelte";

  const win = getCurrentWindow();
  let maximized = $state(false);
  win.isMaximized().then((m) => (maximized = m));
  win.onResized(async () => (maximized = await win.isMaximized()));

  const folderIcon = $derived(extMeta.get("//dir")?.icon ?? null);

  // Drag a tab horizontally to reorder, like Explorer/browsers.
  let drag = $state<{ id: number; x: number; moved: boolean } | null>(null);

  let tabCtx = $state<{ x: number; y: number; id: number } | null>(null);
  function onTabContext(e: MouseEvent, id: number) {
    e.preventDefault();
    tabCtx = { x: e.clientX, y: e.clientY, id };
  }

  function tabDown(e: MouseEvent, id: number) {
    if (e.button === 1) {
      closeTab(id);
      return;
    }
    if (e.button !== 0) return;
    switchTab(id);
    drag = { id, x: e.clientX, moved: false };
  }
  function tabDragMove(e: MouseEvent) {
    if (!drag) return;
    if (!drag.moved && Math.abs(e.clientX - drag.x) < 5) return;
    drag.moved = true;
    const els = [...document.querySelectorAll(".titlebar .tab")] as HTMLElement[];
    const from = tabs.findIndex((t) => t.id === drag!.id);
    const over = els.findIndex((el) => {
      const r = el.getBoundingClientRect();
      return e.clientX >= r.left && e.clientX <= r.right;
    });
    if (over >= 0 && over !== from) {
      const [t] = tabs.splice(from, 1);
      tabs.splice(over, 0, t);
    }
  }
</script>

<svelte:window
  onmousemove={tabDragMove}
  onmouseup={() => (drag = null)}
  onmousedown={(e) => { if (tabCtx && !(e.target as HTMLElement).closest(".menu")) tabCtx = null; }}
/>

{#if tabCtx}
  <div class="menu" style="left:{tabCtx.x}px; top:{tabCtx.y}px">
    <button class="menu-item" onclick={() => { duplicateTab(tabCtx!.id); tabCtx = null; }}>
      <span class="glyph">&#xE8C8;</span> Duplicate tab
    </button>
    <div class="menu-sep"></div>
    <button class="menu-item" onclick={() => { closeTab(tabCtx!.id); tabCtx = null; }}>
      <span class="glyph">&#xE8BB;</span> Close tab
    </button>
    <button class="menu-item" disabled={tabs.length < 2} onclick={() => { closeOtherTabs(tabCtx!.id); tabCtx = null; }}>
      Close other tabs
    </button>
    <button class="menu-item" disabled={tabs.findIndex((t) => t.id === tabCtx!.id) === tabs.length - 1} onclick={() => { closeTabsRight(tabCtx!.id); tabCtx = null; }}>
      Close tabs to the right
    </button>
  </div>
{/if}

<div class="titlebar" data-tauri-drag-region>
  {#each tabs as t (t.id)}
    <div
      class="tab"
      class:active={t.id === app.activeTab}
      role="tab"
      tabindex="-1"
      aria-selected={t.id === app.activeTab}
      onmousedown={(e) => tabDown(e, t.id)}
      oncontextmenu={(e) => onTabContext(e, t.id)}
    >
      {#if folderIcon}
        <img src={folderIcon} alt="" width="16" height="16" />
      {:else}
        <span class="glyph tabglyph">&#xE8B7;</span>
      {/if}
      <span class="tabname">{t.path ? folderName(t.path) : "Home"}</span>
      <button class="tabclose" onclick={(e) => { e.stopPropagation(); closeTab(t.id); }} title="Close tab (Ctrl+W)">
        <span class="glyph">&#xE8BB;</span>
      </button>
    </div>
  {/each}
  <button class="newtab" title="New tab (Ctrl+T)" onclick={() => newTab()}>
    <span class="glyph">&#xE710;</span>
  </button>
  <div class="spacer" data-tauri-drag-region></div>
  <div class="caption">
    <button class="cap" onclick={() => win.minimize()} title="Minimize">
      <span class="glyph">&#xE921;</span>
    </button>
    <button class="cap" onclick={() => win.toggleMaximize()} title={maximized ? "Restore" : "Maximize"}>
      <span class="glyph">{maximized ? "" : ""}</span>
    </button>
    <button class="cap close" onclick={() => win.close()} title="Close">
      <span class="glyph">&#xE8BB;</span>
    </button>
  </div>
</div>

<style>
  .titlebar {
    height: 40px;
    display: flex;
    align-items: flex-end;
    padding-left: 8px;
    flex-shrink: 0;
    overflow: hidden;
  }
  .tab {
    height: 32px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 0 10px;
    min-width: 120px;
    max-width: 220px;
    flex-shrink: 1;
    border-radius: 8px 8px 0 0;
    border: 1px solid transparent;
    border-bottom: none;
    margin-right: 1px;
  }
  .tab.active {
    background: var(--card);
    border-color: var(--stroke);
  }
  .tab:not(.active):hover {
    background: var(--hover);
  }
  .tabglyph {
    font-size: 16px;
    color: var(--text-2);
  }
  .tabname {
    flex: 1;
    min-width: 0;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tabclose {
    width: 24px;
    height: 24px;
    display: grid;
    place-items: center;
    font-size: 0;
    flex-shrink: 0;
  }
  .tabclose .glyph {
    font-size: 10px;
    color: var(--text-2);
  }
  .tabclose:hover {
    background: var(--hover);
  }
  .newtab {
    width: 28px;
    height: 28px;
    margin: 0 0 2px 4px;
    display: grid;
    place-items: center;
    flex-shrink: 0;
  }
  .newtab .glyph {
    font-size: 12px;
    color: var(--text-3);
  }
  .newtab:hover {
    background: var(--hover);
  }
  .spacer {
    flex: 1;
    height: 100%;
    min-width: 40px;
  }
  .caption {
    display: flex;
    align-self: flex-start;
  }
  .cap {
    width: 46px;
    height: 32px;
    display: grid;
    place-items: center;
    border-radius: 0;
  }
  .cap .glyph {
    font-size: 10px;
  }
  .cap:hover {
    background: var(--hover);
  }
  .cap.close:hover {
    background: var(--close-hover);
    color: #fff;
  }
</style>
