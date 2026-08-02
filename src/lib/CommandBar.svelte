<script lang="ts">
  import {
    app, sel, sortBy, setGroup, refresh, copySel, paste, del, newFolder, startRename, toggleHidden,
    setView, selectAll, loadNewItems, newFile, newItems, extMeta, navigate, SETTINGS,
  } from "./store.svelte";
  import { THEMES, setTheme, toggleCompact, settings, setSetting } from "./settings.svelte";

  let openMenu = $state<"" | "new" | "sort" | "view" | "more">("");
  let themeSubOpen = $state(false);
  let menuX = $state(0);
  let menuY = $state(0);

  const selN = $derived(app.allSelected ? app.total : sel.size);
  const hasSel = $derived(selN > 0);

  function toggle(id: typeof openMenu, ev: MouseEvent) {
    if (openMenu === id) {
      openMenu = "";
      return;
    }
    const r = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    menuX = r.left;
    menuY = r.bottom + 4;
    openMenu = id;
    themeSubOpen = false;
    if (id === "new") loadNewItems();
  }

  const sortItems = [
    ["name", "Name"],
    ["modified", "Date modified"],
    ["type", "Type"],
    ["size", "Size"],
    ["created", "Date created"],
    ["author", "Authors"],
    ["category", "Categories"],
    ["tags", "Tags"],
    ["title", "Title"],
  ] as const;
</script>

<svelte:window onmousedown={(e) => { if (!(e.target as HTMLElement).closest(".menu, .cbtn")) openMenu = ""; }} />

<div class="commandbar">
  <button class="cbtn labeled" onclick={(e) => toggle("new", e)}>
    <span class="glyph accent">&#xE710;</span>
    <span>New</span>
    <span class="glyph chev">&#xE70D;</span>
  </button>

  <div class="divider"></div>

  <button class="cbtn" disabled={!hasSel} onclick={() => copySel(true)} title="Cut (Ctrl+X)">
    <span class="glyph">&#xE8C6;</span>
  </button>
  <button class="cbtn" disabled={!hasSel} onclick={() => copySel(false)} title="Copy (Ctrl+C)">
    <span class="glyph">&#xE8C8;</span>
  </button>
  <button class="cbtn" disabled={!app.canPaste} onclick={paste} title="Paste (Ctrl+V)">
    <span class="glyph">&#xE77F;</span>
  </button>
  <button class="cbtn" disabled={selN !== 1} onclick={startRename} title="Rename (F2)">
    <span class="glyph">&#xE8AC;</span>
  </button>
  <button class="cbtn" disabled title="Share">
    <span class="glyph">&#xE72D;</span>
  </button>
  <button class="cbtn" disabled={!hasSel} onclick={del} title="Delete (Del)">
    <span class="glyph">&#xE74D;</span>
  </button>

  <div class="divider"></div>

  <button class="cbtn labeled" onclick={(e) => toggle("sort", e)}>
    <span class="glyph">&#xE8CB;</span>
    <span>Sort</span>
    <span class="glyph chev">&#xE70D;</span>
  </button>
  <button class="cbtn labeled" onclick={(e) => toggle("view", e)}>
    <span class="glyph">&#xE890;</span>
    <span>View</span>
    <span class="glyph chev">&#xE70D;</span>
  </button>

  <div class="divider"></div>

  <button class="cbtn" onclick={(e) => toggle("more", e)} title="See more">
    <span class="glyph">&#xE712;</span>
  </button>

  <button class="cbtn gear" class:active={app.path === SETTINGS} onclick={() => navigate(SETTINGS)} title="Settings">
    <span class="glyph">&#xE713;</span>
  </button>
</div>

{#if openMenu}
  <div class="menu" style="left:{menuX}px; top:{menuY}px">
    {#if openMenu === "new"}
      <button class="menu-item" onclick={() => { openMenu = ""; newFolder(); }}>
        {#if extMeta.get("//dir")?.icon}
          <img class="micon" src={extMeta.get("//dir")?.icon} alt="" width="16" height="16" />
        {:else}
          <span class="glyph">&#xE8B7;</span>
        {/if}
        Folder
      </button>
      {#if newItems.list?.length}
        <div class="menu-sep"></div>
        <div class="newscroll">
          {#each newItems.list as it (it.ext)}
            <button class="menu-item" onclick={() => { openMenu = ""; newFile(it.ext); }}>
              {#if extMeta.get(it.ext)?.icon}
                <img class="micon" src={extMeta.get(it.ext)?.icon} alt="" width="16" height="16" />
              {:else}
                <span class="glyph">&#xE7C3;</span>
              {/if}
              {it.name}
            </button>
          {/each}
        </div>
      {/if}
    {:else if openMenu === "sort"}
      {#each sortItems as [key, label] (key)}
        <button class="menu-item" onclick={() => { openMenu = ""; sortBy(key, app.sortKey === key ? app.asc : true); }}>
          <span class="check">{app.sortKey === key ? "●" : ""}</span> {label}
        </button>
      {/each}
      <div class="menu-sep"></div>
      <button class="menu-item" onclick={() => { openMenu = ""; sortBy(app.sortKey, true); }}>
        <span class="check">{app.asc ? "●" : ""}</span> Ascending
      </button>
      <button class="menu-item" onclick={() => { openMenu = ""; sortBy(app.sortKey, false); }}>
        <span class="check">{!app.asc ? "●" : ""}</span> Descending
      </button>
      <div class="menu-sep"></div>
      {#each [["none", "Group: None"], ["name", "Group by Name"], ["modified", "Group by Date"], ["type", "Group by Type"], ["size", "Group by Size"]] as [gkey, glabel] (gkey)}
        <button class="menu-item" onclick={() => { openMenu = ""; setGroup(gkey); }}>
          <span class="check">{app.groupKey === gkey ? "●" : ""}</span> {glabel}
        </button>
      {/each}
    {:else if openMenu === "view"}
      {#each [["icons_xl", "Extra large icons"], ["icons_l", "Large icons"], ["icons_m", "Medium icons"], ["icons_s", "Small icons"], ["list", "List"], ["details", "Details"], ["tiles", "Tiles"]] as [vkey, vlabel] (vkey)}
        <button class="menu-item" onclick={() => { openMenu = ""; setView(vkey); }}>
          <span class="check">{app.view === vkey ? "●" : ""}</span> {vlabel}
        </button>
      {/each}
      <button class="menu-item" onclick={() => { openMenu = ""; toggleCompact(); }}>
        <span class="check">{settings.compact ? "●" : ""}</span> Compact view
      </button>
      <div class="menu-sep"></div>
      <button class="menu-item" onclick={() => { openMenu = ""; setSetting("previewPane", !settings.previewPane); }}>
        <span class="check">{settings.previewPane ? "●" : ""}</span> Preview pane
      </button>
      <div class="menu-sep"></div>
      <button class="menu-item" onclick={() => { openMenu = ""; toggleHidden(); }}>
        <span class="check">{settings.showHidden ? "●" : ""}</span> Hidden items
      </button>
      <button class="menu-item" onclick={() => { openMenu = ""; setSetting("showExt", !settings.showExt); }}>
        <span class="check">{settings.showExt ? "●" : ""}</span> File name extensions
      </button>
      <div class="menu-sep"></div>
      <div class="subwrap" role="none" onmouseenter={() => (themeSubOpen = true)} onmouseleave={() => (themeSubOpen = false)}>
        <button class="menu-item wide">
          <span class="glyph">&#xE790;</span>
          <span class="mtext">Theme</span>
          <span class="glyph subchev">&#xE76C;</span>
        </button>
        {#if themeSubOpen}
          <div class="menu submenu">
            {#each THEMES as [tid, tlabel] (tid)}
              <button class="menu-item" onclick={() => { openMenu = ""; setTheme(tid); }}>
                <span class="check">{settings.theme === tid ? "●" : ""}</span> {tlabel}
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {:else if openMenu === "more"}
      <button class="menu-item" onclick={() => { openMenu = ""; selectAll(); }}>
        <span class="glyph">&#xE8B3;</span> Select all
      </button>
      <button class="menu-item" onclick={() => { openMenu = ""; refresh(); }}>
        <span class="glyph">&#xE72C;</span> Refresh
      </button>
      <div class="menu-sep"></div>
      <button class="menu-item" onclick={() => { openMenu = ""; navigate(SETTINGS); }}>
        <span class="glyph">&#xE713;</span> Settings
      </button>
    {/if}
  </div>
{/if}

<style>
  .commandbar {
    height: 44px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 12px;
    flex-shrink: 0;
  }
  .cbtn {
    height: 34px;
    min-width: 38px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 0 8px;
  }
  .cbtn .glyph {
    font-size: 16px;
    color: var(--text-2);
  }
  .cbtn:disabled .glyph {
    color: var(--text-disabled);
  }
  .cbtn .accent {
    color: var(--accent);
  }
  .cbtn .chev {
    font-size: 9px;
    color: var(--text-3);
  }
  .cbtn.labeled {
    padding: 0 10px;
  }
  .cbtn:hover:not(:disabled) {
    background: var(--hover);
  }
  .gear {
    margin-left: auto;
  }
  .gear.active {
    background: var(--sel);
  }
  .gear.active .glyph {
    color: var(--accent);
  }
  .divider {
    width: 1px;
    height: 22px;
    background: var(--stroke-strong);
    margin: 0 6px;
    flex-shrink: 0;
  }
  .newscroll {
    max-height: 55vh;
    overflow-y: auto;
  }
  .micon {
    flex-shrink: 0;
  }
  .subwrap {
    position: relative;
  }
  .menu-item.wide {
    justify-content: space-between;
    gap: 8px;
  }
  .mtext {
    flex: 1;
    text-align: left;
    white-space: nowrap;
  }
  .subchev {
    font-size: 10px;
    color: var(--text-3);
  }
  .submenu {
    position: absolute;
    left: calc(100% - 4px);
    top: -4px;
    max-height: 60vh;
    overflow-y: auto;
  }
</style>
