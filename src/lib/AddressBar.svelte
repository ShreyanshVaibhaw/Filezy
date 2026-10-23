<script lang="ts">
  import { msg } from "./i18n.svelte";
  import { menuKeys } from "./menukeys";
  import { invoke } from "@tauri-apps/api/core";
  import { app, navigate, openPath, back, forward, up, refresh, setFilter, clearSearch, startSearch, rememberSearch, folderName, joinPath, can, THIS_PC, HOME, RECYCLE, SETTINGS, NETWORK } from "./store.svelte";
  import { settings } from "./settings.svelte";
  import { crumbs, displayPath, isArchive, isUnc, isVirtual, isWsl } from "./paths";
  import { isShellLocation } from "./shellplaces";

  let editing = $state(false);
  let editValue = $state("");
  let editInput: HTMLInputElement | undefined = $state();
  let searchInput: HTMLInputElement | undefined = $state();
  // Recent searches drop down from the empty search box, as in Explorer.
  let recentOpen = $state(false);
  let recentList: HTMLDivElement | undefined = $state();
  const recent = $derived(settings.searchHistory.slice(0, 8));
  function pickRecent(q: string) {
    recentOpen = false;
    searchValue = q;
    app.filter = q;
    startSearch(q, true);
  }
  function recentKeys(e: KeyboardEvent) {
    const items = [...(recentList?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "ArrowDown") items[Math.min(items.length - 1, i + 1)]?.focus();
    else if (e.key === "ArrowUp") (i <= 0 ? searchInput : items[i - 1])?.focus();
    else if (e.key === "Escape") {
      recentOpen = false;
      searchInput?.focus();
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }
  let searchValue = $state("");

  // The box follows the tab's search: cleared by navigation, refilled when a tab
  // with a search is reselected.
  $effect(() => {
    searchValue = app.filter;
  });

  const segments = $derived.by(() => {
    if (!app.path) return [] as { name: string; path: string }[];
    if (isShellLocation(app.path)) return [{ name: folderName(app.path), path: app.path }];
    if (app.path === THIS_PC) return [{ name: "This PC", path: THIS_PC }];
    if (app.path === HOME) return [{ name: "Home", path: HOME }];
    if (app.path === RECYCLE) return [{ name: "Recycle Bin", path: RECYCLE }];
    if (app.path === SETTINGS) return [{ name: "Settings", path: SETTINGS }];
    if (app.path === NETWORK) return [{ name: "Network", path: NETWORK }];
    const segs = crumbs(app.path).map((c) => ({ ...c, name: c.name.endsWith(":") ? diskName(c.name) : c.name }));
    // A computer and its shares sit under Network, as in Explorer.
    return isUnc(app.path) && !isWsl(app.path) ? [{ name: "Network", path: NETWORK }, ...segs] : segs;
  });

  function diskName(letter: string): string {
    const d = app.drives.find((d) => d.path.startsWith(letter));
    return d ? d.name : letter;
  }

  // Explorer collapses long paths: hidden ancestors go behind a chevron menu.
  const MAX_CRUMBS = 3;
  const hiddenSegs = $derived(segments.length > MAX_CRUMBS ? segments.slice(0, -MAX_CRUMBS) : []);
  const shownSegs = $derived(segments.length > MAX_CRUMBS ? segments.slice(-MAX_CRUMBS) : segments);
  let overflowOpen = $state(false);
  let overflowX = $state(0);
  let overflowY = $state(0);
  function toggleOverflow(ev: MouseEvent) {
    const r = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    overflowX = r.left;
    overflowY = r.bottom + 6;
    overflowOpen = !overflowOpen;
  }

  // Explorer's per-segment dropdown: the chevron after a crumb lists that folder's subfolders.
  let crumbMenu = $state<{ x: number; y: number; items: { name: string; path: string }[] } | null>(null);
  async function openCrumbMenu(path: string, ev: MouseEvent) {
    const r = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    if (crumbMenu) {
      crumbMenu = null;
      return;
    }
    const items =
      path === THIS_PC
        ? app.drives.map((d) => ({ name: d.name, path: d.path }))
        : (await listDirs(path)).map(
            (n) => ({ name: n, path: joinPath(path, n) }),
          );
    if (!items.length) return;
    crumbMenu = { x: r.left, y: r.bottom + 6, items };
  }

  export function focus() {
    startEdit();
  }

  /** Ctrl+F, Ctrl+E, F3: put the caret in the search box. */
  export function focusSearch() {
    if (!can("search") && !app.search) return;
    searchInput?.focus();
    searchInput?.select();
  }

  function listDirs(path: string): Promise<string[]> {
    return invoke<string[]>("list_dirs", {
      path,
      showHidden: settings.showHidden,
      showProtected: settings.showProtected,
    }).catch(() => [] as string[]);
  }

  function startEdit() {
    editValue = displayPath(app.path);
    editing = true;
    suggestions = [];
    setTimeout(() => {
      editInput?.focus();
      editInput?.select();
    });
  }

  /// Go where the address text points: a folder opens in this tab and a file
  /// opens in its app, as typed into Explorer's address bar.
  async function commitEdit(path?: string) {
    editing = false;
    suggestions = [];
    const v = (path ?? editValue).trim();
    if (!v || v === app.path || v === displayPath(app.path)) return;
    if (isVirtual(v)) return void navigate(v);
    try {
      const r = await invoke<{ kind: string; path: string }>("resolve_path", { input: v });
      if (r.kind === "file") await openPath(r.path);
      else await navigate(r.path);
    } catch (e) {
      app.status = String(e);
    }
  }

  // Path autocomplete: complete the segment after the last backslash.
  let suggestions = $state<string[]>([]);
  let sugSel = $state(-1);
  let sugSeq = 0;
  async function updateSuggestions() {
    const v = editValue;
    const cut = v.lastIndexOf("\\");
    if (cut < 2 || v.startsWith("::")) {
      suggestions = [];
      return;
    }
    const parent = cut === 2 ? v.slice(0, 3) : v.slice(0, cut);
    const prefix = v.slice(cut + 1).toLowerCase();
    const seq = ++sugSeq;
    const kids = await listDirs(parent);
    if (seq !== sugSeq || !editing) return;
    suggestions = kids
      .filter((k) => k.toLowerCase().startsWith(prefix))
      .slice(0, 8)
      .map((k) => joinPath(parent, k));
    sugSel = -1;
  }
  function editKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      commitEdit(sugSel >= 0 ? suggestions[sugSel] : undefined);
    } else if (e.key === "Escape") {
      editing = false;
      suggestions = [];
    } else if (e.key === "ArrowDown" && suggestions.length) {
      sugSel = (sugSel + 1) % suggestions.length;
      e.preventDefault();
    } else if (e.key === "ArrowUp" && suggestions.length) {
      sugSel = (sugSel - 1 + suggestions.length) % suggestions.length;
      e.preventDefault();
    } else if (e.key === "Tab" && suggestions.length) {
      editValue = suggestions[sugSel >= 0 ? sugSel : 0];
      updateSuggestions();
      e.preventDefault();
    }
  }
</script>

<div class="addressrow">
  <button class="navbtn" onclick={back} disabled={app.histIdx <= 0} title={msg("Back (Alt + Left)")} aria-label={msg("Back (Alt + Left)")}>
    <span class="glyph" aria-hidden="true">&#xE72B;</span>
  </button>
  <button class="navbtn" onclick={forward} disabled={app.histIdx >= app.histLen - 1} title={msg("Forward (Alt + Right)")} aria-label={msg("Forward (Alt + Right)")}>
    <span class="glyph" aria-hidden="true">&#xE72A;</span>
  </button>
  <button class="navbtn" onclick={up} title={msg("Up (Alt + Up)")} aria-label={msg("Up (Alt + Up)")}>
    <span class="glyph" aria-hidden="true">&#xE74A;</span>
  </button>
  <button class="navbtn" onclick={() => refresh()} title={msg("Refresh (F5)")} aria-label={msg("Refresh (F5)")}>
    <span class="glyph" aria-hidden="true">&#xE72C;</span>
  </button>

  <div
    class="addressbox"
    class:editing
    role="textbox"
    tabindex="-1"
    aria-label={msg("Address")}
    onclick={(e) => { if (e.target === e.currentTarget) startEdit(); }}
    onkeydown={(e) => { if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) startEdit(); }}
  >
    {#if editing}
      <input
        bind:this={editInput}
        bind:value={editValue}
        onkeydown={editKeydown}
        oninput={updateSuggestions}
        onblur={() => setTimeout(() => { editing = false; suggestions = []; }, 150)}
        spellcheck="false"
      />
      {#if suggestions.length}
        <div class="sugmenu">
          {#each suggestions as sug, i (sug)}
            <button
              class="menu-item"
              class:sugactive={i === sugSel}
              onmousedown={(e) => { e.preventDefault(); commitEdit(sug); }}
            >
              <span class="glyph" aria-hidden="true">&#xE8B7;</span>{sug}
            </button>
          {/each}
        </div>
      {/if}
    {:else}
      <span class="glyph pcicon" aria-hidden="true">&#xE977;</span>
      {#if hiddenSegs.length}
        <button class="crumb chevbtn" onclick={toggleOverflow} title={msg("Previous locations")} aria-label={msg("Previous locations")}>
          <span class="glyph" aria-hidden="true">&#xE712;</span>
        </button>
      {/if}
      <button class="crumbchevbtn" onclick={(e) => openCrumbMenu(THIS_PC, e)} title={msg("This PC")} aria-label={msg("This PC")}>
        <span class="glyph crumbchev" aria-hidden="true">&#xE76C;</span>
      </button>
      {#each shownSegs as seg (seg.path)}
        <button
          class="crumb"
          onclick={() => navigate(seg.path)}
          data-drop={isVirtual(seg.path) || isArchive(seg.path) ? undefined : seg.path}
          data-drop-label={seg.name}
        >
          {seg.name}
        </button>
        <!-- Home, Settings and the like have no folders to list. -->
        {#if !isVirtual(seg.path) || seg.path === THIS_PC || isShellLocation(seg.path)}
          <button class="crumbchevbtn" disabled={isShellLocation(seg.path)} aria-label={msg("Folders in {name}", { name: seg.name })} title={isShellLocation(seg.path) ? msg("Open device folders from the list.") : msg("Folders in {name}", { name: seg.name })} onclick={(e) => openCrumbMenu(seg.path, e)}>
            <span class="glyph crumbchev" aria-hidden="true">&#xE76C;</span>
          </button>
        {/if}
      {/each}
      <div class="crumbfill" role="none" onclick={startEdit}></div>
    {/if}
  </div>

  <div class="searchbox">
    <span class="glyph searchicon" aria-hidden="true">&#xE721;</span>
    <input
      bind:this={searchInput}
      placeholder={"Search " + (app.path ? folderName(app.search?.root ?? app.path) : "")}
      aria-label={msg("Search")}
      disabled={isShellLocation(app.path)}
      title={isShellLocation(app.path) ? msg("Search is unavailable in device folders.") : undefined}
      bind:value={searchValue}
      oninput={() => {
        recentOpen = false;
        setFilter(searchValue);
      }}
      onfocus={() => (recentOpen = !searchValue)}
      onclick={() => (recentOpen = !searchValue)}
      onblur={() => {
        if (app.search) rememberSearch(searchValue);
        setTimeout(() => {
          if (!recentList?.contains(document.activeElement)) recentOpen = false;
        }, 0);
      }}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          // Search now instead of after the typing delay.
          recentOpen = false;
          startSearch(searchValue, true);
        } else if (e.key === "Escape") {
          recentOpen = false;
          searchValue = "";
          clearSearch();
        } else if (e.key === "ArrowDown" && recentOpen && recent.length) {
          e.preventDefault();
          recentList?.querySelector("button")?.focus();
        }
      }}
    />
    {#if recentOpen && recent.length}
      <div class="menu recentdrop" role="listbox" aria-label={msg("Recent searches")} bind:this={recentList} onkeydown={recentKeys} tabindex="-1">
        <div class="recenthdr">{msg("Recent searches")}</div>
        {#each recent as r (r)}
          <button class="menu-item" role="option" aria-selected="false" onclick={() => pickRecent(r)} onblur={() => setTimeout(() => {
            if (!recentList?.contains(document.activeElement) && document.activeElement !== searchInput) recentOpen = false;
          }, 0)}>
            <span class="glyph" aria-hidden="true">&#xE81C;</span>{r}
          </button>
        {/each}
      </div>
    {/if}
  </div>
</div>

{#if crumbMenu}
  <div class="menu crumbdrop" style="left:{crumbMenu.x}px; top:{crumbMenu.y}px" use:menuKeys={() => (crumbMenu = null)}>
    {#each crumbMenu.items as it (it.path)}
      <button class="menu-item" onclick={() => { crumbMenu = null; navigate(it.path); }}>
        <span class="glyph" aria-hidden="true">&#xE8B7;</span>{it.name}
      </button>
    {/each}
  </div>
{/if}

{#if overflowOpen}
  <div class="menu" style="left:{overflowX}px; top:{overflowY}px" use:menuKeys={() => (overflowOpen = false)}>
    {#each [...hiddenSegs].reverse() as seg (seg.path)}
      <button class="menu-item" onclick={() => { overflowOpen = false; navigate(seg.path); }}>
        <span class="glyph" aria-hidden="true">&#xE8B7;</span>{seg.name}
      </button>
    {/each}
  </div>
{/if}

<svelte:window onmousedown={(e) => {
  const t = e.target as HTMLElement;
  if (overflowOpen && !t.closest(".menu, .chevbtn")) overflowOpen = false;
  if (crumbMenu && !t.closest(".crumbdrop, .crumbchevbtn")) crumbMenu = null;
}} />

<style>
  .addressrow {
    height: 48px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 12px 0 6px;
    flex-shrink: 0;
  }
  .navbtn {
    width: 36px;
    height: 32px;
    display: grid;
    place-items: center;
    flex-shrink: 0;
  }
  .navbtn .glyph {
    font-size: 15px;
  }
  .navbtn:hover:not(:disabled) {
    background: var(--hover);
  }
  .addressbox {
    position: relative;
    flex: 1;
    min-width: 120px;
    height: 34px;
    margin: 0 6px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 6px;
    background: var(--input-bg);
    border: 1px solid var(--stroke);
    border-bottom-color: var(--stroke-strong);
    border-radius: 5px;
    overflow: hidden;
  }
  .addressbox input {
    flex: 1;
    height: 100%;
    padding: 0 4px;
    user-select: text;
  }
  .pcicon {
    font-size: 16px;
    color: var(--text-2);
    padding: 0 6px;
  }
  .crumb {
    height: 26px;
    padding: 0 8px;
    font-size: 14px;
    white-space: nowrap;
    color: var(--text);
  }
  .crumb:hover {
    background: var(--hover);
  }
  .crumbchev {
    font-size: 10px;
    color: var(--text-3);
    padding: 0 2px;
  }
  .crumbchevbtn {
    height: 26px;
    padding: 0 2px;
  }
  .crumbchevbtn:hover {
    background: var(--hover);
  }
  .crumbdrop {
    max-height: 50vh;
    overflow-y: auto;
  }
  .chevbtn {
    padding: 0 6px;
  }
  .chevbtn .glyph {
    font-size: 12px;
    color: var(--text-2);
  }
  .crumbfill {
    flex: 1;
    align-self: stretch;
  }
  .addressbox.editing {
    overflow: visible; /* the suggestion dropdown hangs below */
  }
  .sugmenu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    z-index: 1000;
    background: var(--menu-bg);
    border: 1px solid var(--stroke-strong);
    border-radius: 8px;
    padding: 4px;
    box-shadow: 0 8px 20px rgba(0, 0, 0, 0.35);
    overflow: hidden;
  }
  .sugmenu .menu-item.sugactive {
    background: var(--sel);
  }
  .searchbox {
    position: relative;
    width: 240px;
    height: 34px;
    display: flex;
    align-items: center;
    padding: 0 10px;
    background: var(--input-bg);
    border: 1px solid var(--stroke);
    border-bottom-color: var(--stroke-strong);
    border-radius: 5px;
    flex-shrink: 0;
  }
  .recentdrop {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    min-width: 0;
  }
  .recenthdr {
    padding: 6px 12px 4px;
    font-size: 12px;
    color: var(--text-3);
  }
  .searchbox .searchicon {
    font-size: 14px;
    color: var(--text-3);
    margin-right: 8px;
  }
  .searchbox input {
    flex: 1;
    min-width: 0;
    user-select: text;
  }
  .searchbox input::placeholder {
    color: var(--text-3);
  }
  @media (max-width: 900px) {
    .searchbox {
      width: 160px;
    }
  }
  @media (max-width: 600px) {
    .addressrow { flex-wrap: wrap; height: auto; min-height: 80px; row-gap: 4px; padding: 4px 8px; }
    .addressbox { min-width: 120px; margin-right: 0; }
    .searchbox { width: 100%; height: 30px; }
  }
</style>
