<!-- The item menu (right-click on items) and the background menu: Filezy's
     own entries, then the shell's, and Windows' full menu behind "Show more
     options". Built-in verbs picked anywhere run Filezy's versions, so undo
     and the message bar keep working. -->
<script lang="ts">
  import { msg } from "./i18n.svelte";
  import { menuKeys, fitMenu } from "./menukeys";
  import {
    app, rowAt, sel, openEntry, refresh, selectAll, isSelected, clearSel, withSel, firstSel, refreshClipState, entryPath,
    isPinned, can, folderName, isFavorite, setFavorite,
  } from "./store.svelte";
  import { loadNewItems, newItems, extMeta, isKept } from "./rowmeta.svelte";
  import {
    copySel, paste, del, newFolder, newFile, startRename, openInNewTabs,
    openInNewWindow, openFileLocation, createShortcuts, pasteShortcuts, shareSel, propertiesSel, pinSelected, sendSelTo,
  } from "./ops.svelte";
  import { checksumsSel, compareSel, sizesSel, analyzeFolder } from "./tools.svelte";
  import { otherTabWindows } from "./tabtransfer";
  import { openSelection } from "./selection.svelte";
  import { settings, menuOn, anyMenuOn } from "./settings.svelte";
  import ShellItems from "./ShellItems.svelte";
  import { nativeMenuAt, OUR_VERBS, streamShellMenu, type ShellItem, type ShellMenu } from "./shellmenu";
  import { invoke } from "@tauri-apps/api/core";
  import { isArchive, isArchiveName, isServer, isVirtual } from "./paths";
  import { compressSel, extractAll, extractSelected } from "./archive.svelte";

  // x, y: where the menu sits; at: where the click was (Windows' own menu opens there).
  let ctx = $state<{ x: number; y: number; at: { x: number; y: number }; onRow: boolean } | null>(null);
  // The shell's entries for the item menu: null while loading. ctxNote says why
  // there are none, when the shell couldn't or wouldn't build them.
  let ctxItems = $state<ShellItem[] | null>(null);
  let ctxNote = $state("");
  let ctxSession = 0;
  let ctxSeq = 0;
  // Share is offered only when every selected item is a file, as in Explorer.
  let ctxFilesOnly = $state(false);

  // The four icons in the top strip of the item menu.
  const STRIP_IDS = ["file.cut", "file.copy", "file.rename", "file.delete"];
  const ROW_IDS = [
    ...STRIP_IDS, "file.open", "file.opennewtab", "file.opennewwindow", "file.openlocation", "file.openwith", "file.extractall",
    "file.keep", "file.freeup", "file.pin", "file.favorite", "file.compress", "file.copypath", "file.shortcut", "file.share", "file.properties", "file.moreoptions",
    "file.sendto", "file.checksum", "file.compare", "file.size",
  ];
  // Inside an archive: look, open and extract; nothing in it changes yet.
  const inArchive = $derived(isArchive(app.path));
  const BLANK_IDS = ["blank.refresh", "blank.paste", "blank.pasteshortcut", "blank.new", "blank.selectall", "blank.terminal", "blank.sizes"];
  // Folders other Filezy windows show: Copy to / Move to targets (dual pane).
  let otherFolders = $state<string[]>([]);

  function onContext(ev: { clientX: number; clientY: number; shiftKey?: boolean; preventDefault(): void }, i: number | null) {
    ev.preventDefault();
    // Shift+right-click: Windows' own menu straight away, as in Explorer.
    if (ev.shiftKey && i !== null && menuOn("file.moreoptions") && !inArchive) {
      if (!isSelected(i)) {
        clearSel();
        sel.add(i);
        app.anchor = i;
        app.focus = i;
      }
      return void moreOptions({ x: ev.clientX, y: ev.clientY }, true);
    }
    // Nothing left to show: don't open an empty box. Shell extensions can still
    // fill the item menu, so only the blank menu is fully suppressed here.
    if (i === null && !anyMenuOn(BLANK_IDS)) return;
    if (i !== null && !anyMenuOn(ROW_IDS) && !settings.shellExtras) return;
    if (i !== null && !isSelected(i)) {
      clearSel();
      sel.add(i);
      app.anchor = i;
      app.focus = i;
    }
    const x = ev.clientX;
    const y = ev.clientY;
    ctx = { x, y, at: { x: ev.clientX, y: ev.clientY }, onRow: i !== null };
    otherFolders = [...new Set(otherTabWindows().map((w) => w.path))].filter((p) => p && !isVirtual(p) && !isArchive(p) && p.toLowerCase() !== app.path.toLowerCase());
    ctxItems = null;
    ctxNote = "";
    const seq = ++ctxSeq;
    if (i !== null) {
      // One loaded row answers at once; larger selections ask the backend.
      const one = !app.allSelected && sel.size === 1 ? rowAt(i) : undefined;
      ctxFilesOnly = !!one && !one.is_dir;
      if (!one) {
        withSel((s) => invoke<{ count: number; files: number }>("selection_stats", { sel: s }))
          .then((st) => seq === ctxSeq && (ctxFilesOnly = st.files === st.count))
          .catch(() => {});
      }
    }
    if (i === null || inArchive) {
      // Blank area: our own Win11-style menu (shell background verbs need Explorer's view host).
      // An archive's entries aren't files the shell can make a menu for.
      ctxItems = [];
      blankNewOpen = false;
      loadNewItems();
      return;
    }
    if (!settings.shellExtras) {
      // Extensions switched off entirely: skip the multi-second shell call.
      ctxItems = [];
      return;
    }
    // The native menu shows instantly; shell-extension entries stream in when
    // QueryContextMenu (which loads every extension DLL) finishes. The backend
    // builds it for every selected item, fetched or not, in any folder.
    streamShellMenu(withSel((s) => invoke<ShellMenu>("shell_menu", { sel: s })), OUR_VERBS, () => seq === ctxSeq && !!ctx, (items, session, note) => {
      ctxItems = items;
      ctxSession = session;
      ctxNote = note;
    });
  }

  /// Run a shell verb Filezy implements itself. False when it isn't one.
  async function runVerb(v: string): Promise<boolean> {
    const first = firstSel();
    const fe = first === undefined ? undefined : rowAt(first);
    switch (v) {
      case "open":
        await openSelection();
        return true;
      case "opennewwindow":
      case "opennewprocess":
        await openInNewWindow();
        return true;
      case "opennewtab":
        await openInNewTabs();
        return true;
      case "cut":
      case "copy":
        await copySel(v === "cut");
        return true;
      case "copyaspath":
        await withSel((sel) => invoke("copy_as_path", { sel }));
        await refreshClipState();
        return true;
      case "delete":
        await del();
        return true;
      case "rename":
        await startRename();
        return true;
      // Pin verbs act on Filezy's Quick access, not Explorer's.
      case "pintohome":
      case "unpinfromhome":
        await pinSelected(v === "pintohome");
        return true;
      // Windows' Add to Favorites for files: Filezy's Favorites on Home.
      case "pintohomefile":
        await favoriteSel(true);
        return true;
      case "properties":
        await propertiesSel();
        return true;
      case "openas":
        if (fe) await invoke("open_with", { path: entryPath(fe) });
        return true;
      case "link":
        await createShortcuts();
        return true;
      case "windows.modernshare":
        await shareSel();
        return true;
      case "makeavailableoffline":
      case "makeavailableonline":
        await cloudPin(v === "makeavailableoffline");
        return true;
    }
    return false;
  }

  /// Add the selected files to Favorites on Home, or take them off.
  async function favoriteSel(on: boolean) {
    const items = await withSel((sel) => invoke<{ path: string; is_dir: boolean }[]>("sel_items", { sel })).catch(() => []);
    await setFavorite(items.filter((it) => !it.is_dir).map((it) => it.path), on);
  }

  /// Always keep on this device (a toggle) or Free up space, for the selection.
  /// Windows sets the pin state and the provider does the rest; the folder's
  /// watcher then shows each item's new Status as it changes.
  async function cloudPin(keep: boolean) {
    try {
      await withSel((sel) => invoke("cloud_pin", { sel, keep }));
    } catch (e) {
      app.status = String(e);
    }
  }

  /// Windows' own context menu for the selection ("Show more options"), with
  /// everything extensions add. Built-in commands picked there still run
  /// Filezy's versions, so undo and the message bar keep working.
  async function moreOptions(at: { x: number; y: number }, extended = false) {
    ctx = null;
    const { x, y } = await nativeMenuAt(at);
    try {
      const verb = await withSel((sel) =>
        invoke<string | null>("shell_menu_native", {
          sel,
          x,
          y,
          extended,
          handled: [...OUR_VERBS].filter((v) => v !== "paste"),
        }),
      );
      if (verb) await runVerb(verb);
    } catch (e) {
      app.status = String(e);
    } finally {
      setTimeout(() => {
        refresh();
        refreshClipState();
      }, 400);
    }
  }

  // Standard verbs run through our native implementations; only third-party
  // extension verbs go through IContextMenu::InvokeCommand (shell-built-in verbs
  // fail without Explorer's view/site chain).
  async function shellInvoke(it: ShellItem) {
    ctx = null;
    try {
      if (await runVerb((it.verb ?? "").toLowerCase())) return;
      await invoke("shell_menu_invoke", { session: ctxSession, id: it.id });
    } catch (e) {
      app.status = String(e);
    } finally {
      setTimeout(() => {
        refresh();
        refreshClipState();
      }, 400);
    }
  }

  // Blank-menu "New >" submenu state
  let blankNewOpen = $state(false);

  export function open(ev: { clientX: number; clientY: number; shiftKey?: boolean; preventDefault(): void }, i: number | null) {
    onContext(ev, i);
  }
</script>

<svelte:window onmousedown={(e) => { if (ctx && !(e.target as HTMLElement).closest(".menu")) ctx = null; }} />

{#if ctx}
  <div class="menu ctxmenu" style="left:{ctx.x}px; top:{ctx.y}px" use:menuKeys={() => (ctx = null)} use:fitMenu={ctx}>
    {#if ctx.onRow && anyMenuOn(STRIP_IDS) && !inArchive}
      <div class="strip">
        {#if menuOn("file.cut") && can("change")}
          <button title={msg("Cut")} aria-label={msg("Cut")} onclick={() => { ctx = null; copySel(true); }}><span class="glyph" aria-hidden="true">&#xE8C6;</span></button>
        {/if}
        {#if menuOn("file.copy")}
          <button title={msg("Copy")} aria-label={msg("Copy")} onclick={() => { ctx = null; copySel(false); }}><span class="glyph" aria-hidden="true">&#xE8C8;</span></button>
        {/if}
        {#if menuOn("file.rename") && can("change")}
          <button title={msg("Rename")} aria-label={msg("Rename")} onclick={() => { ctx = null; startRename(); }}><span class="glyph" aria-hidden="true">&#xE8AC;</span></button>
        {/if}
        {#if menuOn("file.delete") && can("change")}
          <button title={msg("Delete")} aria-label={msg("Delete")} onclick={() => { ctx = null; del(); }}><span class="glyph" aria-hidden="true">&#xE74D;</span></button>
        {/if}
      </div>
      <div class="menu-sep"></div>
    {/if}
    {#if ctx.onRow}
      {@const fi = firstSel()}
      {@const fe = fi !== undefined ? rowAt(fi) : undefined}
      {@const multi = app.allSelected ? app.total : sel.size}
      {#if menuOn("file.open")}
        <button class="menu-item" onclick={() => { ctx = null; if (fi !== undefined) openEntry(fi); }}>
          <span class="glyph" aria-hidden="true">&#xE8E5;</span>{msg("Open")}
        </button>
      {/if}
      {#if fe?.is_dir && menuOn("file.opennewtab")}
        <button class="menu-item" onclick={() => { ctx = null; openInNewTabs(); }}>
          <span class="glyph" aria-hidden="true">&#xE8A7;</span>{msg("Open in new tab")}
        </button>
      {/if}
      {#if fe?.is_dir && multi === 1 && menuOn("file.opennewwindow")}
        <button class="menu-item" onclick={() => { ctx = null; openInNewWindow(); }}>
          <span class="glyph" aria-hidden="true">&#xE737;</span>{msg("Open in new window")}
        </button>
      {/if}
      {#if fe && app.search && multi === 1 && menuOn("file.openlocation")}
        <button class="menu-item" onclick={() => { ctx = null; openFileLocation(entryPath(fe)); }}>
          <span class="glyph" aria-hidden="true">&#xE838;</span>{msg("Open file location")}
        </button>
      {/if}
      {#if fe && !fe.is_dir && multi === 1 && menuOn("file.openwith") && !inArchive}
        <button class="menu-item" onclick={() => { ctx = null; invoke("open_with", { path: entryPath(fe) }).catch((e) => (app.status = String(e))); }}>
          <span class="glyph" aria-hidden="true">&#xE7AC;</span>{msg("Open with")}
        </button>
      {/if}
      {#if inArchive && menuOn("file.extractall")}
        <button class="menu-item" onclick={() => { ctx = null; extractSelected(); }}>
          <span class="glyph" aria-hidden="true">&#xE8B7;</span>{msg("Extract...")}
        </button>
        <button class="menu-item" onclick={() => { ctx = null; extractAll(); }}>
          <span class="glyph" aria-hidden="true">&#xE8B7;</span>{msg("Extract all...")}
        </button>
      {:else if fe && !fe.is_dir && multi === 1 && isArchiveName(fe.name) && menuOn("file.extractall")}
        <button class="menu-item" onclick={() => { ctx = null; extractAll(entryPath(fe)); }}>
          <span class="glyph" aria-hidden="true">&#xE8B7;</span>{msg("Extract all...")}
        </button>
      {/if}
      {#if app.cloud && !inArchive && menuOn("file.keep")}
        {@const kept = !!fe && isKept(fe)}
        <button class="menu-item" role="menuitemcheckbox" aria-checked={kept} onclick={() => { ctx = null; cloudPin(true); }}>
          <span class="glyph" aria-hidden="true">{kept ? "" : ""}</span>{msg("Always keep on this device")}
        </button>
      {/if}
      {#if app.cloud && !inArchive && menuOn("file.freeup")}
        <button class="menu-item" onclick={() => { ctx = null; cloudPin(false); }}>
          <span class="glyph" aria-hidden="true">&#xE753;</span>{msg("Free up space")}
        </button>
      {/if}
      {#if fe?.is_dir && menuOn("file.pin") && !inArchive}
        {#if isPinned(entryPath(fe))}
          <button class="menu-item" onclick={() => { ctx = null; pinSelected(false); }}>
            <span class="glyph" aria-hidden="true">&#xE77A;</span>{msg("Unpin from Quick access")}
          </button>
        {:else}
          <button class="menu-item" onclick={() => { ctx = null; pinSelected(true); }}>
            <span class="glyph" aria-hidden="true">&#xE840;</span>{msg("Pin to Quick access")}
          </button>
        {/if}
      {/if}
      {#if ctxFilesOnly && menuOn("file.favorite") && !inArchive}
        {@const fav = !!fe && isFavorite(entryPath(fe))}
        <button class="menu-item" onclick={() => { const on = !fav; ctx = null; favoriteSel(on); }}>
          <span class="glyph" aria-hidden="true">{fav ? "" : ""}</span>{fav ? "Remove from Favorites" : "Add to Favorites"}
        </button>
      {/if}
      {#if menuOn("file.compress") && can("change")}
        <button class="menu-item" onclick={() => { ctx = null; compressSel(); }}>
          <span class="glyph" aria-hidden="true">&#xF012;</span>{msg("Compress to ZIP file")}
        </button>
      {/if}
      {#if menuOn("file.copypath") && !inArchive}
        <button class="menu-item" onclick={() => { ctx = null; withSel((sel) => invoke("copy_as_path", { sel })).then(refreshClipState).catch((e) => (app.status = String(e))); }}>
          <span class="glyph" aria-hidden="true">&#xE8C8;</span>{msg("Copy as path")}
        </button>
      {/if}
      {#if menuOn("file.sendto") && !inArchive}
        {#each otherFolders as dest (dest)}
          <button class="menu-item" onclick={() => { ctx = null; sendSelTo(dest, false); }} title={dest}>
            <span class="glyph" aria-hidden="true">&#xE8C8;</span>{msg("Copy to {name}", { name: folderName(dest) })}
          </button>
          {#if can("change")}
            <button class="menu-item" onclick={() => { ctx = null; sendSelTo(dest, true); }} title={dest}>
              <span class="glyph" aria-hidden="true">&#xE8F0;</span>{msg("Move to {name}", { name: folderName(dest) })}
            </button>
          {/if}
        {/each}
      {/if}
      {#if !inArchive && menuOn("file.checksum") && ctxFilesOnly}
        <button class="menu-item" onclick={() => { ctx = null; checksumsSel(); }}>
          <span class="glyph" aria-hidden="true">&#xE8D7;</span>{msg("Checksums")}
        </button>
      {/if}
      {#if !inArchive && menuOn("file.compare") && ctxFilesOnly && multi === 2}
        <button class="menu-item" onclick={() => { ctx = null; compareSel(); }}>
          <span class="glyph" aria-hidden="true">&#xE8F1;</span>{msg("Compare files")}
        </button>
      {/if}
      {#if !inArchive && menuOn("file.size")}
        <button class="menu-item" onclick={() => { ctx = null; sizesSel(); }}>
          <span class="glyph" aria-hidden="true">&#xEDA2;</span>{msg("Calculate size")}
        </button>
      {/if}
      {#if menuOn("file.shortcut") && can("change")}
        <button class="menu-item" onclick={() => { ctx = null; createShortcuts(); }}>
          <span class="glyph" aria-hidden="true">&#xE71B;</span>{msg("Create shortcut")}
        </button>
      {/if}
      {#if ctxFilesOnly && menuOn("file.share") && !inArchive}
        <button class="menu-item" onclick={() => { ctx = null; shareSel(); }}>
          <span class="glyph" aria-hidden="true">&#xE72D;</span>{msg("Share")}
        </button>
      {/if}
      {#if menuOn("file.properties") && !inArchive}
        <button class="menu-item" onclick={() => { ctx = null; propertiesSel(); }}>
          <span class="glyph" aria-hidden="true">&#xE90F;</span>{msg("Properties")}
        </button>
      {/if}
      {#if ctxItems === null}
        <div class="menu-sep"></div>
        <div class="menuload">{msg("Loading…")}</div>
      {:else if ctxItems.length}
        <div class="menu-sep"></div>
        <!-- Extensions can add dozens of entries: they scroll on their own, so the
             native entries and Show more options stay in view. -->
        <div class="extras">
          <ShellItems items={ctxItems} run={shellInvoke} />
        </div>
      {:else if ctxNote && settings.shellExtras}
        <div class="menu-sep"></div>
        <div class="menunote" role="note">{ctxNote}</div>
      {/if}
      {#if menuOn("file.moreoptions") && !inArchive}
        <div class="menu-sep"></div>
        <button class="menu-item" onclick={() => ctx && moreOptions(ctx.at)}>
          <span class="glyph" aria-hidden="true">&#xE712;</span>{msg("Show more options")}
        </button>
      {/if}
    {:else}
      {#if menuOn("blank.refresh")}
        <button class="menu-item" onclick={() => { ctx = null; refresh(); }}>
          <span class="glyph" aria-hidden="true">&#xE72C;</span>{msg("Refresh")}
        </button>
      {/if}
      <!-- Search results, a server's shares and archives take no new items. -->
      {#if menuOn("blank.paste") && can("paste")}
        <button class="menu-item" disabled={!app.canPaste} onclick={() => { ctx = null; paste(); }}>
          <span class="glyph" aria-hidden="true">&#xE77F;</span>{msg("Paste")}
        </button>
      {/if}
      {#if menuOn("blank.pasteshortcut") && can("paste")}
        <button class="menu-item" disabled={!app.canPaste} onclick={() => { ctx = null; pasteShortcuts(); }}>
          <span class="glyph" aria-hidden="true">&#xE71B;</span>{msg("Paste shortcut")}
        </button>
      {/if}
      {#if menuOn("blank.new") && can("new")}
        <div class="menu-sep"></div>
        <div class="subwrap" role="none" onmouseenter={() => (blankNewOpen = true)} onmouseleave={() => (blankNewOpen = false)}>
          <button class="menu-item wide">
            <span class="glyph" aria-hidden="true">&#xE710;</span>
            <span class="mtext">{msg("New")}</span>
            <span class="glyph subchev" aria-hidden="true">&#xE76C;</span>
          </button>
          {#if blankNewOpen}
            <div class="menu submenu" use:menuKeys={() => (blankNewOpen = false)}>
              <button class="menu-item wide" onclick={() => { ctx = null; newFolder(); }}>
                {#if extMeta.get("//dir")?.icon}
                  <img src={extMeta.get("//dir")?.icon} alt="" width="16" height="16" />
                {:else}
                  <span class="glyph" aria-hidden="true">&#xE8B7;</span>
                {/if}
                <span class="mtext">{msg("Folder")}</span>
              </button>
              {#if newItems.list?.length}
                <div class="menu-sep"></div>
                {#each newItems.list as it (it.ext)}
                  <button class="menu-item wide" onclick={() => { ctx = null; newFile(it.ext); }}>
                    {#if extMeta.get(it.ext)?.icon}
                      <img src={extMeta.get(it.ext)?.icon} alt="" width="16" height="16" />
                    {:else}
                      <span class="glyph" aria-hidden="true">&#xE7C3;</span>
                    {/if}
                    <span class="mtext">{it.name}</span>
                  </button>
                {/each}
              {/if}
            </div>
          {/if}
        </div>
      {/if}
      {#if menuOn("blank.selectall")}
        <button class="menu-item" onclick={() => { ctx = null; selectAll(); }}>
          <span class="glyph" aria-hidden="true">&#xE8B3;</span>{msg("Select all")}
        </button>
      {/if}
      {#if inArchive && menuOn("file.extractall")}
        <div class="menu-sep"></div>
        <button class="menu-item" onclick={() => { ctx = null; extractAll(); }}>
          <span class="glyph" aria-hidden="true">&#xE8B7;</span>{msg("Extract all...")}
        </button>
      {/if}
      {#if menuOn("blank.sizes") && !inArchive && !app.search && !isServer(app.path)}
        <div class="menu-sep"></div>
        <button class="menu-item" onclick={() => { ctx = null; analyzeFolder(); }}>
          <span class="glyph" aria-hidden="true">&#xEDA2;</span>{msg("Folder sizes")}
        </button>
      {/if}
      {#if menuOn("blank.terminal") && !inArchive && !isServer(app.path)}
        <div class="menu-sep"></div>
        <button class="menu-item" onclick={() => { ctx = null; invoke("open_terminal", { dir: app.path }).catch((e) => (app.status = String(e))); }}>
          <span class="glyph" aria-hidden="true">&#xE756;</span>{msg("Open in Terminal")}
        </button>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .ctxmenu {
    min-width: min(260px, calc(100vw - 8px));
    max-width: 340px;
  }
  .extras {
    max-height: 45vh;
    overflow-y: auto;
  }
  .menuload,
  .menunote {
    padding: 10px 14px;
    font-size: 12px;
    color: var(--text-3);
  }
  .strip {
    display: flex;
    justify-content: space-evenly;
    padding: 2px;
  }
  .strip button {
    width: 40px;
    height: 34px;
    display: grid;
    place-items: center;
  }
  .strip button:hover {
    background: var(--hover);
  }
  .strip .glyph {
    font-size: 16px;
    color: var(--text-2);
  }
</style>
