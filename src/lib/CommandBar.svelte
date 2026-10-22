<script lang="ts">
  import { msg } from "./i18n.svelte";
  import { menuKeys } from "./menukeys";
  import {
    app, sel, sortBy, setGroup, refresh, toggleHidden, setView, selectAll, loadNewItems, newItems,
    extMeta, navigate, can, SETTINGS, THIS_PC, NETWORK, loadDrives, applyFilter, setSearchScope, clearSearchHistory, startSearch, clearSearch,
    type SearchScope,
  } from "./store.svelte";
  import ShellItems from "./ShellItems.svelte";
  import type { ShellItem } from "./shellmenu";
  import { filterOf } from "./searchquery";
  import { extractAll, extractTarget } from "./archive.svelte";
  import { baseName, isArchive } from "./paths";
  import { isShellLocation } from "./shellplaces";
  import { copySel, paste, del, newFolder, newFile, startRename, shareSel } from "./ops.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { selectNone, invertSelection } from "./selection.svelte";
  import { THEMES, setTheme, toggleCompact, settings, setSetting, showPane } from "./settings.svelte";
  import { saved, saveSearch, runSearch, saveWorkspace, openWorkspace } from "./saved.svelte";
  import { analyzeFolder } from "./tools.svelte";
  import { isVirtual } from "./paths";

  /// Dual pane: this window on the left half of the screen, a second one on the right.
  function dualPane() {
    invoke("split_view", { path: isVirtual(app.path) ? null : app.path }).catch((e) => (app.status = String(e)));
  }
  let workSubOpen = $state(false);

  let openMenu = $state<"" | "new" | "sort" | "view" | "more" | "search">("");
  let themeSubOpen = $state(false);
  let menuX = $state(0);
  let menuY = $state(0);

  const selN = $derived(app.allSelected ? app.total : sel.size);
  const hasSel = $derived(selN > 0);

  // Share works for files only, as in Explorer: count the folders in the selection.
  let filesOnly = $state(false);
  let shareSeq = 0;
  $effect(() => {
    void selN;
    void app.rev;
    const seq = ++shareSeq;
    if (!selN) {
      filesOnly = false;
      return;
    }
    invoke<{ count: number; files: number }>("selection_stats", { sel: app.allSelected ? { rev: app.rev, all: true } : { rev: app.rev, idx: [...sel] } })
      .then((s) => seq === shareSeq && (filesOnly = s.files === s.count))
      .catch(() => {});
  });

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
    workSubOpen = false;
    if (id === "new") loadNewItems();
  }

  // ---- Search options (Explorer's Search tab) ----
  const SCOPES: [SearchScope, string][] = [
    ["folder", "Current folder"],
    ["subfolders", "All subfolders"],
    ["drive", "This drive"],
    ["all", "All drives"],
  ];
  const DATES: [string, string][] = [
    ["today", "Today"], ["yesterday", "Yesterday"], ["this week", "This week"], ["last week", "Last week"],
    ["this month", "This month"], ["last month", "Last month"], ["this year", "This year"], ["last year", "Last year"],
  ];
  const KINDS: [string, string][] = [
    ["document", "Document"], ["folder", "Folder"], ["picture", "Picture"], ["video", "Video"], ["music", "Music"],
    ["archive", "Archive"], ["program", "Program"], ["text", "Text"],
  ];
  const SIZES: [string, string][] = [
    ["empty", "Empty (0 KB)"], ["tiny", "Tiny (0 - 16 KB)"], ["small", "Small (16 KB - 1 MB)"], ["medium", "Medium (1 - 128 MB)"],
    ["large", "Large (128 MB - 1 GB)"], ["huge", "Huge (1 - 4 GB)"], ["gigantic", "Gigantic (> 4 GB)"],
  ];
  const searching = $derived(!!app.search || !!app.filter.trim());
  // The archive Extract all would act on: the one shown, or the one selected.
  const extractFrom = $derived(extractTarget());

  /// Windows' Map Network Drive dialog; the drives show the new one at once.
  async function mapDrive() {
    if (await invoke<boolean>("map_drive").catch((e) => ((app.status = String(e)), false))) await loadDrives();
  }
  const inArchive = $derived(isArchive(app.path));

  /// The Search options menu as menu entries (ShellItems draws the submenus);
  /// each entry's id indexes its action. Filters edit the query text, so the
  /// search box always shows the whole search.
  let actions: (() => void)[] = [];
  function searchMenu(): ShellItem[] {
    actions = [];
    const q = app.filter;
    const base = { verb: null, sep: false, disabled: false, sub: null };
    const entry = (text: string, act: () => void, checked?: boolean): ShellItem => {
      actions.push(act);
      return { ...base, id: actions.length - 1, text, checked };
    };
    const sub = (text: string, items: ShellItem[]): ShellItem => ({ ...base, id: -1, text, sub: items });
    const sep: ShellItem = { ...base, id: -1, text: "", sep: true };
    const pick = (key: string, options: [string, string][]) =>
      options.map(([v, label]) => entry(label, () => applyFilter(key, filterOf(q, key) === v ? null : v), filterOf(q, key) === v));
    const recent = settings.searchHistory.slice(0, 10);
    return [
      ...SCOPES.map(([s, label]) => entry(label, () => setSearchScope(s), app.searchScope === s)),
      sep,
      sub("Date modified", pick("datemodified", DATES)),
      sub("Kind", pick("kind", KINDS)),
      sub("Size", pick("size", SIZES)),
      sub("Date created", pick("datecreated", DATES)),
      entry("Clear filters", () => applyFilter("*", null)),
      sep,
      sub(
        "Recent searches",
        recent.length
          ? recent.map((h) => entry(h, () => { app.filter = h; startSearch(h, true); }))
          : [{ ...base, id: -1, text: "No recent searches", disabled: true }],
      ),
      entry("Clear search history", clearSearchHistory),
      sep,
      entry("Save search...", saveSearch),
      sub(
        "Saved searches",
        saved.searches.length
          ? saved.searches.map((s) => entry(s.name, () => runSearch(s)))
          : [{ ...base, id: -1, text: "No saved searches", disabled: true }],
      ),
      sep,
      entry("Close search", () => { app.filter = ""; clearSearch(); }),
    ];
  }
  function runSearchItem(it: ShellItem) {
    openMenu = "";
    actions[it.id]?.();
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
  <button class="cbtn labeled" disabled={!can("new")} onclick={(e) => toggle("new", e)}>
    <span class="glyph accent">&#xE710;</span>
    <span>{msg("New")}</span>
    <span class="glyph chev">&#xE70D;</span>
  </button>

  <div class="divider"></div>

  <button class="cbtn" disabled={!hasSel || !can("change")} onclick={() => copySel(true)} title={msg("Cut (Ctrl+X)")} aria-label={msg("Cut (Ctrl+X)")}>
    <span class="glyph">&#xE8C6;</span>
  </button>
  <button class="cbtn" disabled={!hasSel || inArchive} onclick={() => copySel(false)} title={msg("Copy (Ctrl+C)")} aria-label={msg("Copy (Ctrl+C)")}>
    <span class="glyph">&#xE8C8;</span>
  </button>
  <button class="cbtn" disabled={!app.canPaste || !can("paste")} onclick={paste} title={msg("Paste (Ctrl+V)")} aria-label={msg("Paste (Ctrl+V)")}>
    <span class="glyph">&#xE77F;</span>
  </button>
  <button class="cbtn" disabled={!hasSel || !can("change")} onclick={startRename} title={msg("Rename (F2)")} aria-label={msg("Rename (F2)")}>
    <span class="glyph">&#xE8AC;</span>
  </button>
  <button
    class="cbtn"
    disabled={!filesOnly || !can("change")}
    onclick={shareSel}
    title={hasSel && !filesOnly ? "Share works for files, not folders" : "Share"}
    aria-label={msg("Share")}
  >
    <span class="glyph">&#xE72D;</span>
  </button>
  <button class="cbtn" disabled={!hasSel || !can("change")} onclick={() => del()} title={msg("Delete (Del)")} aria-label={msg("Delete (Del)")}>
    <span class="glyph">&#xE74D;</span>
  </button>

  <div class="divider"></div>

  <button class="cbtn labeled" disabled={isShellLocation(app.path)} onclick={(e) => toggle("sort", e)}>
    <span class="glyph">&#xE8CB;</span>
    <span>{msg("Sort")}</span>
    <span class="glyph chev">&#xE70D;</span>
  </button>
  <button class="cbtn labeled" disabled={isShellLocation(app.path)} onclick={(e) => toggle("view", e)}>
    <span class="glyph">&#xE890;</span>
    <span>{msg("View")}</span>
    <span class="glyph chev">&#xE70D;</span>
  </button>
  {#if extractFrom}
    <button class="cbtn labeled" onclick={() => extractAll(extractFrom)} title="Extract all files from {baseName(extractFrom)}">
      <span class="glyph">&#xE8B7;</span>
      <span>{msg("Extract all")}</span>
    </button>
  {/if}
  {#if searching}
    <button class="cbtn labeled" onclick={(e) => toggle("search", e)}>
      <span class="glyph">&#xE71C;</span>
      <span>{msg("Search options")}</span>
      <span class="glyph chev">&#xE70D;</span>
    </button>
  {/if}

  <div class="divider"></div>

  <button class="cbtn" onclick={(e) => toggle("more", e)} title={msg("See more")} aria-label={msg("See more")}>
    <span class="glyph">&#xE712;</span>
  </button>

  <button class="cbtn gear" class:active={app.path === SETTINGS} onclick={() => navigate(SETTINGS)} title={msg("Settings")} aria-label={msg("Settings")}>
    <span class="glyph">&#xE713;</span>
  </button>
</div>

{#if openMenu}
  <div class="menu" style="left:{menuX}px; top:{menuY}px" use:menuKeys={() => (openMenu = "")}>
    {#if openMenu === "new"}
      <button class="menu-item" onclick={() => { openMenu = ""; newFolder(); }}>
        {#if extMeta.get("//dir")?.icon}
          <img class="micon" src={extMeta.get("//dir")?.icon} alt="" width="16" height="16" />
        {:else}
          <span class="glyph">&#xE8B7;</span>
        {/if}
        {msg("Folder")}
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
    {:else if openMenu === "search"}
      <ShellItems items={searchMenu()} run={runSearchItem} />
    {:else if openMenu === "sort"}
      {#each sortItems as [key, label] (key)}
        <button class="menu-item" onclick={() => { openMenu = ""; sortBy(key, app.sortKey === key ? app.asc : true); }}>
          <span class="check">{app.sortKey === key ? "●" : ""}</span> {msg(label)}
        </button>
      {/each}
      <div class="menu-sep"></div>
      <button class="menu-item" onclick={() => { openMenu = ""; sortBy(app.sortKey, true); }}>
        <span class="check">{app.asc ? "●" : ""}</span> {msg("Ascending")}
      </button>
      <button class="menu-item" onclick={() => { openMenu = ""; sortBy(app.sortKey, false); }}>
        <span class="check">{!app.asc ? "●" : ""}</span> {msg("Descending")}
      </button>
      <div class="menu-sep"></div>
      {#each [["none", "Group: None"], ["name", "Group by Name"], ["modified", "Group by Date"], ["type", "Group by Type"], ["size", "Group by Size"], ["month", "Group by Month"]] as [gkey, glabel] (gkey)}
        <button class="menu-item" onclick={() => { openMenu = ""; setGroup(gkey); }}>
          <span class="check">{app.groupKey === gkey ? "●" : ""}</span> {msg(glabel)}
        </button>
      {/each}
    {:else if openMenu === "view"}
      {#each [["icons_xl", "Extra large icons"], ["icons_l", "Large icons"], ["icons_m", "Medium icons"], ["icons_s", "Small icons"], ["list", "List"], ["details", "Details"], ["content", "Content"], ["tiles", "Tiles"], ["gallery", "Gallery"]] as [vkey, vlabel] (vkey)}
        <button class="menu-item" onclick={() => { openMenu = ""; setView(vkey); }}>
          <span class="check">{app.view === vkey ? "●" : ""}</span> {msg(vlabel)}
        </button>
      {/each}
      <button class="menu-item" onclick={() => { openMenu = ""; toggleCompact(); }}>
        <span class="check">{settings.compact ? "●" : ""}</span> {msg("Compact view")}
      </button>
      <div class="menu-sep"></div>
      <button class="menu-item" onclick={() => { openMenu = ""; dualPane(); }}>
        <span class="glyph">&#xE8A9;</span> {msg("Dual pane (side-by-side windows)")}
      </button>
      <button class="menu-item" onclick={() => { openMenu = ""; showPane(settings.previewPane ? null : "preview"); }}>
        <span class="check">{settings.previewPane ? "●" : ""}</span> {msg("Preview pane")}
      </button>
      <button class="menu-item" onclick={() => { openMenu = ""; showPane(settings.detailsPane ? null : "details"); }}>
        <span class="check">{settings.detailsPane ? "●" : ""}</span> {msg("Details pane")}
      </button>
      <div class="menu-sep"></div>
      <button class="menu-item" onclick={() => { openMenu = ""; toggleHidden(); }}>
        <span class="check">{settings.showHidden ? "●" : ""}</span> {msg("Hidden items")}
      </button>
      <button class="menu-item" onclick={() => { openMenu = ""; setSetting("showExt", !settings.showExt); }}>
        <span class="check">{settings.showExt ? "●" : ""}</span> {msg("File name extensions")}
      </button>
      <button class="menu-item" onclick={() => { openMenu = ""; setSetting("checkboxes", !settings.checkboxes); }}>
        <span class="check">{settings.checkboxes ? "●" : ""}</span> {msg("Item check boxes")}
      </button>
      <div class="menu-sep"></div>
      <div class="subwrap" role="none" onmouseenter={() => (themeSubOpen = true)} onmouseleave={() => (themeSubOpen = false)}>
        <button class="menu-item wide">
          <span class="glyph">&#xE790;</span>
          <span class="mtext">{msg("Theme")}</span>
          <span class="glyph subchev">&#xE76C;</span>
        </button>
        {#if themeSubOpen}
          <div class="menu submenu" use:menuKeys={() => (themeSubOpen = false)}>
            {#each THEMES as [tid, tlabel] (tid)}
              <button class="menu-item" onclick={() => { openMenu = ""; setTheme(tid); }}>
                <span class="check">{settings.theme === tid ? "●" : ""}</span> {tlabel}
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {:else if openMenu === "more"}
      <button class="menu-item" disabled={!can("select")} onclick={() => { openMenu = ""; selectAll(); }}>
        <span class="glyph">&#xE8B3;</span> {msg("Select all")}
      </button>
      <button class="menu-item" disabled={!can("select")} onclick={() => { openMenu = ""; selectNone(); }}>
        <span class="glyph">&#xE8E6;</span> {msg("Select none")}
      </button>
      <button class="menu-item" disabled={!can("select")} onclick={() => { openMenu = ""; invertSelection(); }}>
        <span class="glyph">&#xE8B6;</span> {msg("Invert selection")}
      </button>
      <button class="menu-item" onclick={() => { openMenu = ""; refresh(); }}>
        <span class="glyph">&#xE72C;</span> {msg("Refresh")}
      </button>
      <button class="menu-item" disabled={!can("new") || inArchive || !!app.search} onclick={() => { openMenu = ""; analyzeFolder(); }}>
        <span class="glyph">&#xEDA2;</span> {msg("Folder sizes")}
      </button>
      <div class="menu-sep"></div>
      <button class="menu-item" onclick={() => { openMenu = ""; saveWorkspace(); }}>
        <span class="glyph">&#xE74E;</span> {msg("Save tabs as workspace...")}
      </button>
      <div class="subwrap" role="none" onmouseenter={() => (workSubOpen = true)} onmouseleave={() => (workSubOpen = false)}>
        <button class="menu-item wide" aria-haspopup="menu" aria-expanded={workSubOpen}>
          <span class="glyph">&#xE737;</span>
          <span class="mtext">{msg("Workspaces")}</span>
          <span class="glyph subchev">&#xE76C;</span>
        </button>
        {#if workSubOpen}
          <div class="menu submenu" use:menuKeys={() => (workSubOpen = false)}>
            {#each saved.workspaces as w (w.name)}
              <button class="menu-item" onclick={() => { openMenu = ""; openWorkspace(w); }} title={w.tabs.join(", ")}>
                <span class="glyph">&#xE737;</span> {w.name}
              </button>
            {:else}
              <button class="menu-item" disabled>{msg("No saved workspaces")}</button>
            {/each}
          </div>
        {/if}
      </div>
      {#if app.path === THIS_PC || app.path === NETWORK}
        <div class="menu-sep"></div>
        <button class="menu-item" onclick={() => { openMenu = ""; mapDrive(); }}>
          <span class="glyph">&#xE8CE;</span> {msg("Map network drive...")}
        </button>
        <button class="menu-item" onclick={() => { openMenu = ""; invoke("disconnect_drive", { path: null }).then(loadDrives).catch((e) => (app.status = String(e))); }}>
          <span class="glyph">&#xE8CD;</span> {msg("Disconnect network drive...")}
        </button>
      {/if}
      <div class="menu-sep"></div>
      <button class="menu-item" onclick={() => { openMenu = ""; navigate(SETTINGS); }}>
        <span class="glyph">&#xE713;</span> {msg("Settings")}
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
    overflow-x: auto;
  }
  .commandbar > :global(*) { flex-shrink: 0; }
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
