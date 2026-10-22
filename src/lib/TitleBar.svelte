<script lang="ts">
  import { msg } from "./i18n.svelte";
  import { menuKeys, fitMenu } from "./menukeys";
  import { tick } from "svelte";
  import { dropTab, otherTabWindows, transferTab } from "./tabtransfer";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { nativeCaption } from "./caption";
  import {
    app, tabs, extMeta, folderName, newTab, switchTab, closeTab,
    duplicateTab, closeOtherTabs, closeTabsRight, moveTabToWindow, shellIcon,
    HOME, THIS_PC, RECYCLE, SETTINGS, RECYCLE_GUID,
  } from "./store.svelte";
  import { menuOn, anyMenuOn } from "./settings.svelte";
  import { dropHover } from "./drag";
  import { isVirtual } from "./paths";

  const win = getCurrentWindow();
  let maximized = $state(false);
  win.isMaximized().then((m) => (maximized = m));
  win.onResized(async () => (maximized = await win.isMaximized()));

  const folderIcon = $derived(extMeta.get("//dir")?.icon ?? null);

  // Virtual pages get their own icon in the tab strip, like Explorer's Home tab.
  const VIRTUAL_GLYPHS: Record<string, string> = {
    "": "\uE80F",
    [HOME]: "\uE80F",
    [THIS_PC]: "\uE977",
    [SETTINGS]: "\uE713",
  };

  // Drag a tab horizontally to reorder, like Explorer/browsers.
  let drag = $state<{ id: number; x: number; y: number; moved: boolean } | null>(null);
  let tabOverflow = $state(false);
  let targets = $state<{ id: string; name: string }[]>([]);
  $effect(() => {
    const id = app.activeTab;
    tick().then(() => document.querySelector<HTMLElement>(`[data-tab="${id}"]`)?.scrollIntoView({ block: "nearest", inline: "nearest" }));
  });

  const TAB_IDS = ["tab.duplicate", "tab.movewindow", "tab.close", "tab.closeothers", "tab.closeright"];

  let tabCtx = $state<{ x: number; y: number; id: number } | null>(null);
  function onTabContext(e: MouseEvent, id: number) {
    e.preventDefault();
    if (!anyMenuOn(TAB_IDS)) return;
    tabCtx = { x: e.clientX, y: e.clientY, id };
    targets = otherTabWindows();
  }

  /// Left/Right/Home/End move between tabs and switch to them; Delete closes one.
  function tabKeys(e: KeyboardEvent) {
    const i = tabs.findIndex((t) => t.id === app.activeTab);
    let next = -1;
    if (e.key === "ArrowRight") next = (i + 1) % tabs.length;
    else if (e.key === "ArrowLeft") next = (i - 1 + tabs.length) % tabs.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = tabs.length - 1;
    else if (e.key === "Delete") {
      closeTab(app.activeTab);
    } else return;
    e.preventDefault();
    e.stopPropagation();
    if (next >= 0) {
      const id = tabs[next].id;
      switchTab(id);
      queueMicrotask(() => document.querySelector<HTMLElement>(`[data-tab="${id}"]`)?.focus());
    }
  }

  function tabDown(e: PointerEvent, id: number) {
    if (e.button === 1) {
      closeTab(id);
      return;
    }
    if (e.button !== 0) return;
    switchTab(id);
    drag = { id, x: e.clientX, y: e.clientY, moved: false };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function tabDragMove(e: MouseEvent) {
    if (!drag) return;
    if (!drag.moved && Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 5) return;
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
  function tabUp(e: PointerEvent) {
    const moving = drag;
    drag = null;
    if (moving?.moved && (e.clientY < 0 || e.clientY > 56 || e.clientX < 0 || e.clientX > innerWidth)) {
      void dropTab(moving.id, e.clientX, e.clientY).catch((error) => app.status = String(error));
    }
  }
</script>

<svelte:window
  onpointermove={tabDragMove}
  onpointerup={tabUp}
  onpointercancel={() => drag = null}
  onmousedown={(e) => { if (!(e.target as HTMLElement).closest(".menu, .tabpicker")) { tabCtx = null; tabOverflow = false; } }}
/>

{#if tabCtx}
  <div class="menu" style="left:{tabCtx.x}px; top:{tabCtx.y}px" use:fitMenu use:menuKeys={() => (tabCtx = null)}>
    {#if menuOn("tab.duplicate")}
      <button class="menu-item" onclick={() => { duplicateTab(tabCtx!.id); tabCtx = null; }}>
        <span class="glyph">&#xE8C8;</span> {msg("Duplicate tab")}
      </button>
    {/if}
    {#if menuOn("tab.movewindow")}
      <button class="menu-item" onclick={() => { moveTabToWindow(tabCtx!.id); tabCtx = null; }}>
        <span class="glyph">&#xE737;</span> {msg("Move to new window")}
      </button>
      {#each targets as target (target.id)}
        <button class="menu-item targetwindow" title={target.name} onclick={() => { const id = tabCtx!.id; tabCtx = null; transferTab(id, target.id); }}>
          {msg("Move to window:")} {target.name}
        </button>
      {/each}
    {/if}
    {#if menuOn("tab.duplicate") || menuOn("tab.movewindow")}
      <div class="menu-sep"></div>
    {/if}
    {#if menuOn("tab.close")}
      <button class="menu-item" onclick={() => { closeTab(tabCtx!.id); tabCtx = null; }}>
        <span class="glyph">&#xE8BB;</span> {msg("Close tab")}
      </button>
    {/if}
    {#if menuOn("tab.closeothers")}
      <button class="menu-item" disabled={tabs.length < 2} onclick={() => { closeOtherTabs(tabCtx!.id); tabCtx = null; }}>
        {msg("Close other tabs")}
      </button>
    {/if}
    {#if menuOn("tab.closeright")}
      <button class="menu-item" disabled={tabs.findIndex((t) => t.id === tabCtx!.id) === tabs.length - 1} onclick={() => { closeTabsRight(tabCtx!.id); tabCtx = null; }}>
        {msg("Close tabs to the right")}
      </button>
    {/if}
  </div>
{/if}
{#if tabOverflow}
  <div class="menu tabmenu" style="left:8px;top:40px" use:fitMenu use:menuKeys={() => tabOverflow = false}>
    {#each tabs as t (t.id)}
      <button class="menu-item" title={t.path} role="menuitemradio" aria-checked={t.id === app.activeTab} onclick={() => { switchTab(t.id); tabOverflow = false; }}>
        {folderName(t.path) || "Home"}
      </button>
    {/each}
  </div>
{/if}

<div class="titlebar" data-tauri-drag-region>
  <div class="tabs" role="tablist" aria-label={msg("Tabs")} tabindex="-1" onkeydown={tabKeys}>
  {#each tabs as t (t.id)}
    <div
      class="tab"
      class:active={t.id === app.activeTab}
      role="tab"
      tabindex={t.id === app.activeTab ? 0 : -1}
      aria-selected={t.id === app.activeTab}
      data-tab={t.id}
      data-drop={t.path && !isVirtual(t.path) ? t.path : undefined}
      data-drop-label={t.path ? folderName(t.path) : undefined}
      use:dropHover={() => switchTab(t.id)}
      onpointerdown={(e) => { if (!(e.target as HTMLElement).closest(".tabclose")) tabDown(e, t.id); }}
      oncontextmenu={(e) => onTabContext(e, t.id)}
    >
      {#if t.path === RECYCLE && shellIcon(RECYCLE_GUID, 48)}
        <img src={shellIcon(RECYCLE_GUID, 48)} alt="" width="16" height="16" />
      {:else if VIRTUAL_GLYPHS[t.path] || t.path === RECYCLE}
        <span class="glyph tabglyph">{VIRTUAL_GLYPHS[t.path] ?? "\uE74D"}</span>
      {:else if folderIcon}
        <img src={folderIcon} alt="" width="16" height="16" />
      {:else}
        <span class="glyph tabglyph">&#xE8B7;</span>
      {/if}
      <span class="tabname">{t.path ? folderName(t.path) : "Home"}</span>
      <button class="tabclose" tabindex="-1" onclick={(e) => { e.stopPropagation(); closeTab(t.id); }} title={msg("Close tab (Ctrl+W)")} aria-label={msg("Close tab (Ctrl+W)")}>
        <span class="glyph">&#xE8BB;</span>
      </button>
    </div>
  {/each}
  </div>
  <button class="newtab tabpicker" title={msg("All tabs")} aria-label={msg("All tabs")} aria-expanded={tabOverflow} onclick={() => tabOverflow = !tabOverflow}>
    <span class="glyph">&#xE70D;</span>
  </button>
  <button class="newtab" title={msg("New tab (Ctrl+T)")} aria-label={msg("New tab (Ctrl+T)")} onclick={() => newTab()}>
    <span class="glyph">&#xE710;</span>
  </button>
  <div class="spacer" data-tauri-drag-region></div>
  <div class="caption">
    <button class="cap" tabindex="-1" onclick={() => win.minimize()} title={msg("Minimize")} aria-label={msg("Minimize")}>
      <span class="glyph">&#xE921;</span>
    </button>
    <button class="cap" use:nativeCaption={maximized} tabindex="-1" onclick={() => win.toggleMaximize()} title={maximized ? msg("Restore") : msg("Maximize")} aria-label={maximized ? msg("Restore") : msg("Maximize")}>
      <span class="glyph">{maximized ? "" : ""}</span>
    </button>
    <button class="cap close" tabindex="-1" onclick={() => win.close()} title={msg("Close")} aria-label={msg("Close")}>
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
  .tabs {
    display: flex;
    align-items: flex-end;
    min-width: 0;
    flex-shrink: 1;
    height: 100%;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .tabs::-webkit-scrollbar { display: none; }
  .targetwindow, .tabmenu .menu-item { max-width: min(400px, calc(100vw - 16px)); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tabs:focus {
    outline: none;
  }
  .tab:focus-visible {
    outline: 1px solid var(--text-2);
    outline-offset: -2px;
  }
  .tab {
    height: 32px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 0 10px;
    min-width: 120px;
    max-width: 220px;
    flex-shrink: 0;
    width: 180px;
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
    min-width: 24px;
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
  @media (forced-colors: active) {
    .tab.active {
      forced-color-adjust: none;
      background: Highlight;
      color: HighlightText;
    }
    .tab.active * {
      color: HighlightText;
    }
  }
</style>
