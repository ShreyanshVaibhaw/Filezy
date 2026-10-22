<script lang="ts">
  import { msg } from "./i18n.svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import appIcon from "../../app-icon.png";
  import {
    settings, setSetting, setTheme, THEMES,
    menuOn, setMenuOn, resetMenu, setExtraOn, forgetExtras, resetAll,
  } from "./settings.svelte";
  import { MENU_GROUPS } from "./menuitems";
  import { updater, checkForUpdates, downloadAndInstall, restartApp } from "./updater.svelte";
  import { toggleHidden, clearFolderPrefs, folderPrefsCount, refresh, clearRecent, app, HOME, THIS_PC } from "./store.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import Toggle from "./Toggle.svelte";
  import { saved, runSearch, openWorkspace, renameSaved, deleteSaved } from "./saved.svelte";

  // "Open in Filezy" in Explorer's folder menus: the registry says whether it's there.
  let explorerEntry = $state(false);
  invoke<boolean>("explorer_entry").then((v) => (explorerEntry = v)).catch(() => {});
  async function setExplorerEntry(on: boolean) {
    try {
      await invoke("set_explorer_entry", { on });
    } catch (e) {
      app.status = String(e);
    }
    explorerEntry = await invoke<boolean>("explorer_entry").catch(() => false);
  }

  const CATEGORIES = [
    { id: "appearance", label: "Appearance", glyph: "" },
    { id: "files", label: "Files and folders", glyph: "" },
    { id: "menu", label: "Context menu", glyph: "" },
    { id: "about", label: "About", glyph: "" },
  ];

  // Session-only: reopening Settings starts on Appearance, like a fresh page.
  let cat = $state("appearance");
  let version = $state("");
  getVersion()
    .then((v) => (version = v))
    .catch(() => {});

  const title = $derived(CATEGORIES.find((c) => c.id === cat)?.label ?? "");

  const VIEWS = [
    ["details", "Details"],
    ["content", "Content"],
    ["list", "List"],
    ["tiles", "Tiles"],
    ["icons_s", "Small icons"],
    ["icons_m", "Medium icons"],
    ["icons_l", "Large icons"],
    ["icons_xl", "Extra large icons"],
    ["gallery", "Gallery"],
  ];
  const SORTS = [
    ["name", "Name"],
    ["modified", "Date modified"],
    ["type", "Type"],
    ["size", "Size"],
    ["created", "Date created"],
  ];

  // Start page: the two virtual pages, or any folder the user types.
  const startKind = $derived(settings.startPage.startsWith("::") ? settings.startPage : "custom");
  let customStart = $state("");
  // Follow the stored path, including a change made in another window.
  $effect(() => {
    if (!settings.startPage.startsWith("::")) customStart = settings.startPage;
  });
  function setStartKind(v: string) {
    // Picking "A folder" with nothing typed yet stores an empty path, which keeps
    // the folder input on screen; new tabs fall back to Home until it is filled in.
    setSetting("startPage", v === "custom" ? customStart : v);
  }

  // Learned in menu-build order; read alphabetically.
  const seenSorted = $derived([...settings.seenExtras].sort((a, b) => a.localeCompare(b)));

  let resetArmed = $state(false);
  function doResetAll() {
    if (!resetArmed) {
      resetArmed = true;
      return;
    }
    resetAll();
    customStart = "";
    resetArmed = false;
  }

  // Clearing saved folder views is destructive, so the button asks once.
  let clearArmed = $state(false);
  let clearedCount = $state(-1);
  function clearFolders() {
    if (!clearArmed) {
      clearArmed = true;
      return;
    }
    clearedCount = folderPrefsCount();
    clearFolderPrefs();
    clearArmed = false;
  }

  // Clearing Recent files also asks once; it removes Windows' Recent items list.
  let recentArmed = $state(false);
  let recentCleared = $state(-1);
  async function clearRecentFiles() {
    if (!recentArmed) {
      recentArmed = true;
      return;
    }
    recentArmed = false;
    recentCleared = await clearRecent();
  }
</script>

<div class="settings">
  <div class="rail">
    {#each CATEGORIES as c (c.id)}
      <button class="railitem" class:active={cat === c.id} onclick={() => (cat = c.id)} aria-label={msg(c.label)} title={msg(c.label)}>
        <span class="glyph">{c.glyph}</span>
        <span class="raillabel">{msg(c.label)}</span>
      </button>
    {/each}
  </div>

  <div class="content">
    <div class="inner">
      <h1 class="title">{msg(title)}</h1>

      {#if cat === "appearance"}
        <h2 class="section">{msg("Theme")}</h2>
        <div class="themegrid">
          {#each THEMES as [id, label] (id)}
            <!-- Each card carries the theme's own data-theme, so the preview is
                 painted by that theme's variables instead of hardcoded colors. -->
            <button
              class="themecard"
              class:active={settings.theme === id}
              data-theme={id === "system" ? undefined : id}
              title={label}
              onclick={() => setTheme(id)}
            >
              <div class="mock">
                <div class="mocktop"><span class="mocktab"></span></div>
                <div class="mockbody">
                  <div class="mockrail"><i></i><i></i><i></i></div>
                  <div class="mockmain"><b></b><i></i><i></i></div>
                </div>
              </div>
              <span class="themename">
                {label}
                {#if settings.theme === id}<span class="glyph tick">&#xE73E;</span>{/if}
              </span>
            </button>
          {/each}
        </div>

        <h2 class="section">{msg("Layout")}</h2>
        <div class="row">
          <span class="glyph rowicon">&#xE8A9;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Compact view")}</div>
            <div class="rowdesc">{msg("Tighter row spacing in Details and List views")}</div>
          </div>
          <Toggle label={msg("Compact view")} checked={settings.compact} onchange={(v) => setSetting("compact", v)} />
        </div>
        <div class="row">
          <span class="glyph rowicon">&#xE890;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Preview pane")}</div>
            <div class="rowdesc">{msg("Show a preview of the selected file (Alt+P)")}</div>
          </div>
          <Toggle label={msg("Preview pane")} checked={settings.previewPane} onchange={(v) => setSetting("previewPane", v)} />
        </div>
      {:else if cat === "files"}
        <h2 class="section">{msg("Display")}</h2>
        <div class="row">
          <span class="glyph rowicon">&#xE7B3;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Show hidden items")}</div>
            <div class="rowdesc">{msg("Include files and folders marked hidden by Windows")}</div>
          </div>
          <Toggle label={msg("Show hidden items")} checked={settings.showHidden} onchange={toggleHidden} />
        </div>
        <div class="row">
          <span class="glyph rowicon">&#xE7C3;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("File name extensions")}</div>
            <div class="rowdesc">{msg("Show the extension after each file name")}</div>
          </div>
          <Toggle label={msg("File name extensions")} checked={settings.showExt} onchange={(v) => setSetting("showExt", v)} />
        </div>
        <div class="row">
          <span class="glyph rowicon">&#xE73A;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Item check boxes")}</div>
            <div class="rowdesc">{msg("Select items with check boxes as well as clicks")}</div>
          </div>
          <Toggle label={msg("Item check boxes")} checked={settings.checkboxes} onchange={(v) => setSetting("checkboxes", v)} />
        </div>
        <div class="row">
          <span class="glyph rowicon">&#xE72E;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Show protected operating system files")}</div>
            <div class="rowdesc">{msg("Files Windows marks as both hidden and system, such as desktop.ini. Needs hidden items on.")}</div>
          </div>
          <Toggle
            label={msg("Show protected operating system files")}
            checked={settings.showProtected}
            onchange={(v) => { setSetting("showProtected", v); refresh(); }}
          />
        </div>

        <div class="row">
          <span class="glyph rowicon">&#xE8B0;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Single-click to open items")}</div>
            <div class="rowdesc">{msg("Ctrl+click selects without opening. Off by default.")}</div>
          </div>
          <Toggle label={msg("Single-click to open items")} checked={settings.singleClick} onchange={(v) => setSetting("singleClick", v)} />
        </div>
        <h2 class="section">{msg("Default folder view")}</h2>
        <div class="row">
          <span class="glyph rowicon">&#xE890;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Layout")}</div>
            <div class="rowdesc">{msg("Used by folders you have not customized")}</div>
          </div>
          <select class="combo" value={settings.defaultView} onchange={(e) => setSetting("defaultView", e.currentTarget.value)}>
            {#each VIEWS as [v, l] (v)}<option value={v}>{msg(l)}</option>{/each}
          </select>
        </div>
        <div class="row">
          <span class="glyph rowicon">&#xE8CB;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Sort by")}</div>
          </div>
          <select class="combo" value={settings.defaultSort} onchange={(e) => setSetting("defaultSort", e.currentTarget.value)}>
            {#each SORTS as [v, l] (v)}<option value={v}>{msg(l)}</option>{/each}
          </select>
        </div>
        <div class="row">
          <span class="glyph rowicon">&#xE74A;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Ascending order")}</div>
          </div>
          <Toggle label={msg("Ascending order")} checked={settings.defaultAsc} onchange={(v) => setSetting("defaultAsc", v)} />
        </div>
        <div class="row">
          <span class="glyph rowicon">&#xE72C;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Reset folder views")}</div>
            <div class="rowdesc">
              {clearedCount >= 0
                ? `Cleared ${clearedCount} saved folder view${clearedCount === 1 ? "" : "s"}`
                : folderPrefsCount() === 1
                  ? "1 folder has its own saved view and sort"
                  : `${folderPrefsCount()} folders have their own saved view and sort`}
            </div>
          </div>
          <button class="btn" class:danger={clearArmed} onclick={clearFolders}>
            {clearArmed ? "Click again to clear" : "Clear"}
          </button>
        </div>

        <h2 class="section">{msg("Saved searches and workspaces")}</h2>
        {#each saved.searches as s, i (s.name)}
          <div class="row">
            <span class="glyph rowicon">&#xE721;</span>
            <div class="rowlabel">
              <div class="rowname">{s.name}</div>
              <div class="rowdesc" title={s.root}>"{s.query}" in {s.root}</div>
            </div>
            <div class="rowbtns">
              <button class="btn" onclick={() => runSearch(s)} aria-label={`${msg("Search")} ${s.name}`}>{msg("Search")}</button>
              <button class="btn" onclick={() => renameSaved("searches", i)} aria-label={`${msg("Rename")} ${s.name}`}>{msg("Rename")}</button>
              <button class="btn" onclick={() => deleteSaved("searches", i)} aria-label={`${msg("Delete")} ${s.name}`}>{msg("Delete")}</button>
            </div>
          </div>
        {/each}
        {#each saved.workspaces as w, i (w.name)}
          <div class="row">
            <span class="glyph rowicon">&#xE737;</span>
            <div class="rowlabel">
              <div class="rowname">{w.name}</div>
              <div class="rowdesc" title={w.tabs.join("\n")}>{w.tabs.length} {w.tabs.length === 1 ? msg("tab") : msg("tabs")}: {w.tabs.join(", ")}</div>
            </div>
            <div class="rowbtns">
              <button class="btn" onclick={() => openWorkspace(w)} aria-label={`${msg("Open")} ${w.name}`}>{msg("Open")}</button>
              <button class="btn" onclick={() => renameSaved("workspaces", i)} aria-label={`${msg("Rename")} ${w.name}`}>{msg("Rename")}</button>
              <button class="btn" onclick={() => deleteSaved("workspaces", i)} aria-label={`${msg("Delete")} ${w.name}`}>{msg("Delete")}</button>
            </div>
          </div>
        {/each}
        {#if !saved.searches.length && !saved.workspaces.length}
          <div class="row">
            <span class="glyph rowicon">&#xE734;</span>
            <div class="rowlabel">
              <div class="rowname">{msg("Nothing saved yet")}</div>
              <div class="rowdesc">{msg("Save a search from Search options, or this window's tabs from See more > Save tabs as workspace.")}</div>
            </div>
          </div>
        {/if}

        <h2 class="section">{msg("Privacy")}</h2>
        <div class="row">
          <span class="glyph rowicon">&#xE81C;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Clear recent files")}</div>
            <div class="rowdesc">
              {recentCleared >= 0
                ? `Cleared ${recentCleared} recent ${recentCleared === 1 ? "entry" : "entries"}`
                : "The list of files you opened lately, which Home, Explorer and Start show. The files themselves stay."}
            </div>
          </div>
          <button class="btn" class:danger={recentArmed} onclick={clearRecentFiles}>
            {recentArmed ? "Click again to clear" : "Clear"}
          </button>
        </div>

        <h2 class="section">{msg("Windows integration")}</h2>
        <div class="row">
          <span class="glyph rowicon">&#xE8DA;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Open in Filezy from File Explorer")}</div>
            <div class="rowdesc">{msg("Adds \"Open in Filezy\" to Explorer's right-click menu for folders and drives. Explorer stays your file manager; turning this off, or uninstalling Filezy, takes the entry out.")}</div>
          </div>
          <Toggle label={msg("Open in Filezy from File Explorer")} checked={explorerEntry} onchange={setExplorerEntry} />
        </div>

        <h2 class="section">{msg("Startup")}</h2>
        <div class="row">
          <span class="glyph rowicon">&#xE7C4;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Reopen tabs from last time")}</div>
            <div class="rowdesc">{msg("When Filezy starts without a folder to open, it brings back the tabs of the window you used last")}</div>
          </div>
          <Toggle label={msg("Reopen tabs from last time")} checked={settings.restoreTabs} onchange={(v) => setSetting("restoreTabs", v)} />
        </div>
        <div class="row">
          <span class="glyph rowicon">&#xE80F;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Open new tabs and windows on")}</div>
          </div>
          <select class="combo" value={startKind} onchange={(e) => setStartKind(e.currentTarget.value)}>
            <option value={HOME}>{msg("Home")}</option>
            <option value={THIS_PC}>{msg("This PC")}</option>
            <option value="custom">{msg("A folder")}</option>
          </select>
        </div>
        {#if startKind === "custom"}
          <div class="row">
            <span class="glyph rowicon">&#xE8B7;</span>
            <div class="rowlabel">
              <div class="rowname">{msg("Start folder")}</div>
              <div class="rowdesc">{msg("Falls back to Home if the folder is not available")}</div>
            </div>
            <input
              class="pathinput"
              placeholder={msg("D:\\Projects")}
              value={customStart}
              onchange={(e) => { customStart = e.currentTarget.value.trim(); setSetting("startPage", customStart); }}
            />
          </div>
        {/if}
      {:else if cat === "menu"}
        <p class="lede">
          {msg("Switch off the entries you never use and they stop appearing in the right-click menu. Nothing here changes Windows itself.")}
        </p>
        {#each MENU_GROUPS as g (g.key)}
          <h2 class="section">{msg(g.label)}</h2>
          <div class="rowdesc groupdesc">{g.desc}</div>
          {#each g.items as it (it.id)}
            <div class="row compactrow">
              <span class="glyph rowicon">{it.glyph}</span>
              <div class="rowlabel"><div class="rowname">{msg(it.label)}</div></div>
              <Toggle label={msg(it.label)} checked={menuOn(it.id)} onchange={(v) => setMenuOn(it.id, v)} />
            </div>
          {/each}
        {/each}
        <div class="resetrow">
          <button class="btn" onclick={resetMenu} disabled={settings.hiddenMenu.length === 0}>
            {msg("Show all entries again")}
          </button>
          <span class="rowdesc">
            {settings.hiddenMenu.length} {msg("entr")}{settings.hiddenMenu.length === 1 ? "y" : "ies"} {msg("hidden")}
          </span>
        </div>

        <h2 class="section">{msg("Entries from other apps")}</h2>
        <div class="rowdesc groupdesc">
          {msg("Added by installed software through the Windows shell, like archivers and editors")}
        </div>
        <div class="row">
          <span class="glyph rowicon">&#xE71D;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Show entries from other apps")}</div>
            <div class="rowdesc">{msg("Turning this off also skips loading their handlers, so menus open faster")}</div>
          </div>
          <Toggle label={msg("Show entries from other apps")} checked={settings.shellExtras} onchange={(v) => setSetting("shellExtras", v)} />
        </div>
        {#if settings.shellExtras}
          {#if seenSorted.length}
            {#each seenSorted as name (name)}
              <div class="row compactrow">
                <span class="glyph rowicon">&#xE7B8;</span>
                <div class="rowlabel"><div class="rowname">{name}</div></div>
                <Toggle
                  label={name}
                  checked={!settings.hiddenExtras.includes(name)}
                  onchange={(v) => setExtraOn(name, v)}
                />
              </div>
            {/each}
            <div class="resetrow">
              <button class="btn" onclick={forgetExtras}>{msg("Forget this list")}</button>
              <span class="rowdesc">{seenSorted.length} {msg("seen so far")}</span>
            </div>
          {:else}
            <div class="rowdesc groupdesc">
              {msg("This list fills in as you open context menus, since Windows only reveals these entries when a menu is built.")}
            </div>
          {/if}
        {/if}
      {:else if cat === "about"}
        <div class="row">
          <img class="applogo" src={appIcon} alt="" width="40" height="40" />
          <div class="rowlabel">
            <div class="rowname">{msg("Filezy")}</div>
            <div class="rowdesc">{msg("A faster Windows File Explorer")}</div>
          </div>
          <span class="version">{version ? "Version " + version : ""}</span>
        </div>

        <div class="row">
          <span class="glyph rowicon">&#xE896;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Updates")}</div>
            <div class="rowdesc">
              {#if !updater.installed}
                {msg("This copy runs from a folder rather than an installation, so it cannot update itself.")}
              {:else if updater.phase === "checking"}
                {msg("Checking for updates…")}
              {:else if updater.phase === "none"}
                {msg("Filezy is up to date")}
              {:else if updater.phase === "found"}
                {msg("Version")} {updater.version} {msg("is available")}
              {:else if updater.phase === "downloading"}
                {msg("Downloading…")} {updater.percent}%
              {:else if updater.phase === "ready"}
                {msg("Update installed. Restart to finish.")}
              {:else if updater.phase === "error"}
                {updater.message}
              {:else}
                {msg("Updates are downloaded and installed from inside the app")}
              {/if}
            </div>
          </div>
          {#if updater.phase === "found"}
            <button class="btn accentbtn" onclick={downloadAndInstall}>{msg("Download and install")}</button>
          {:else if updater.phase === "ready"}
            <button class="btn accentbtn" onclick={restartApp}>{msg("Restart now")}</button>
          {:else}
            <button
              class="btn"
              disabled={!updater.installed || updater.phase === "checking" || updater.phase === "downloading"}
              onclick={checkForUpdates}
            >
              {msg("Check for updates")}
            </button>
          {/if}
        </div>
        {#if updater.phase === "downloading"}
          <div class="progress"><span style="width:{updater.percent}%"></span></div>
        {/if}
        {#if updater.phase === "found" && updater.notes}
          <div class="rowdesc notes">{updater.notes}</div>
        {/if}
        <div class="row">
          <span class="glyph rowicon">&#xE777;</span>
          <div class="rowlabel">
            <div class="rowname">{msg("Reset all settings")}</div>
            <div class="rowdesc">
              {msg("Theme, layout, folder defaults and every context-menu switch go back to stock")}
            </div>
          </div>
          <button class="btn" class:danger={resetArmed} onclick={doResetAll}>
            {resetArmed ? "Click again to reset" : "Reset"}
          </button>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .settings {
    flex: 1;
    min-height: 0;
    display: flex;
    overflow: hidden;
  }
  .rail {
    width: 220px;
    flex-shrink: 0;
    padding: 16px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
  }
  .railitem {
    height: 36px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 12px;
    border-radius: 4px;
    position: relative;
    text-align: left;
  }
  .railitem:hover {
    background: var(--hover);
  }
  .railitem.active {
    background: var(--sel);
  }
  .railitem.active::before {
    content: "";
    position: absolute;
    left: 0;
    top: 50%;
    transform: translateY(-50%);
    width: 3px;
    height: 16px;
    border-radius: 2px;
    background: var(--accent);
  }
  .railitem .glyph {
    font-size: 16px;
    width: 16px;
    color: var(--text-2);
  }
  .raillabel {
    font-size: 14px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .content {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 8px 24px 32px;
  }
  .inner {
    max-width: 720px;
  }
  .title {
    font-size: 28px;
    font-weight: 600;
    margin: 8px 0 20px;
  }
  .section {
    font-size: 14px;
    font-weight: 600;
    margin: 24px 0 8px;
  }
  .section:first-of-type {
    margin-top: 0;
  }
  .themegrid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(164px, 1fr));
    gap: 10px;
    margin-bottom: 4px;
  }
  .themecard {
    padding: 6px;
    border: 1px solid var(--stroke-strong);
    border-radius: 6px;
    background: var(--mica-fallback);
    display: flex;
    flex-direction: column;
    gap: 6px;
    text-align: left;
  }
  .themecard:hover {
    border-color: var(--text-3);
  }
  .themecard.active {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }
  .themename {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-2);
    padding: 0 2px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tick {
    font-size: 10px;
    color: var(--accent);
  }
  /* Miniature of the app window, drawn entirely from the card's own theme vars. */
  .mock {
    height: 74px;
    border-radius: 4px;
    overflow: hidden;
    background: var(--mica-fallback);
    display: flex;
    flex-direction: column;
  }
  .mocktop {
    height: 14px;
    display: flex;
    align-items: flex-end;
    padding-left: 6px;
  }
  .mocktab {
    width: 34px;
    height: 10px;
    border-radius: 3px 3px 0 0;
    background: var(--card);
    border: 1px solid var(--stroke);
    border-bottom: none;
  }
  .mockbody {
    flex: 1;
    display: flex;
    background: var(--card);
    border-top: 1px solid var(--stroke);
    padding: 6px;
    gap: 6px;
  }
  .mockrail {
    width: 30px;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .mockmain {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .mockrail i,
  .mockmain i {
    height: 4px;
    border-radius: 2px;
    background: var(--text-3);
    display: block;
  }
  .mockmain b {
    height: 5px;
    width: 40%;
    border-radius: 2px;
    background: var(--accent);
    display: block;
  }
  .mockmain i:last-child {
    width: 70%;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 16px;
    min-height: 52px;
    padding: 8px 16px;
    margin-bottom: 4px;
    background: var(--surface);
    border: 1px solid var(--stroke);
    border-radius: 4px;
  }
  .rowicon {
    font-size: 16px;
    width: 20px;
    color: var(--text-2);
    flex-shrink: 0;
  }
  .applogo {
    flex-shrink: 0;
    border-radius: 4px;
  }
  .rowlabel {
    flex: 1;
    min-width: 0;
  }
  .rowname {
    font-size: 14px;
  }
  .rowdesc {
    font-size: 12px;
    color: var(--text-3);
    margin-top: 2px;
    overflow-wrap: anywhere;
  }
  .version {
    font-size: 12px;
    color: var(--text-3);
  }
  .lede {
    font-size: 14px;
    color: var(--text-2);
    margin-bottom: 4px;
    max-width: 60ch;
  }
  .groupdesc {
    margin: -4px 0 8px 2px;
  }
  .compactrow {
    min-height: 44px;
  }
  .resetrow {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 20px;
  }
  .combo,
  .pathinput {
    height: 32px;
    min-width: 160px;
    padding: 0 8px;
    border-radius: 4px;
    border: 1px solid var(--stroke-strong);
    background: var(--input-bg);
    color: var(--text);
    font-family: var(--font);
    font-size: 14px;
    flex-shrink: 0;
  }
  .combo:hover,
  .pathinput:hover {
    background: var(--surface-hover);
  }
  .pathinput {
    min-width: 240px;
  }
  .rowbtns {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }
  @media (max-width: 640px) {
    .rowbtns { flex-direction: column; }
  }
  .btn {
    height: 32px;
    padding: 0 12px;
    font-size: 14px;
    border-radius: 4px;
    border: 1px solid var(--stroke-strong);
    background: var(--input-bg);
    flex-shrink: 0;
  }
  .btn:hover {
    background: var(--surface-hover);
  }
  .btn.danger {
    border-color: var(--close-hover);
    color: var(--close-hover);
  }
  .btn.accentbtn {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--menu-bg);
  }
  .progress {
    height: 4px;
    border-radius: 2px;
    background: var(--surface);
    overflow: hidden;
    margin: 0 0 4px;
  }
  .progress span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 120ms linear;
  }
  .notes {
    margin: 0 0 8px 2px;
    white-space: pre-wrap;
  }
  @media (max-width: 1000px) {
    .rail { width: 52px; padding: 8px 4px; }
    .railitem { padding: 0 12px; flex-shrink: 0; }
    .raillabel { display: none; }
    .content { padding: 8px 12px 24px; }
    .row { flex-wrap: wrap; gap: 8px; padding: 8px; }
    .pathinput { min-width: 0; width: 100%; }
    .combo { min-width: 0; max-width: 100%; }
    .rowlabel { min-width: min(160px, 100%); }
    .title { font-size: 22px; }
  }
</style>
