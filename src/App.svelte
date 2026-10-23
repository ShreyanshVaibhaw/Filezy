<script lang="ts">
  import { msg } from "./lib/i18n.svelte";
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import TitleBar from "./lib/TitleBar.svelte";
  import AddressBar from "./lib/AddressBar.svelte";
  import CommandBar from "./lib/CommandBar.svelte";
  import NavPane from "./lib/NavPane.svelte";
  import FilePane from "./lib/FilePane.svelte";
  import DrivesPane from "./lib/DrivesPane.svelte";
  import NetworkPane from "./lib/NetworkPane.svelte";
  import PreviewPane from "./lib/PreviewPane.svelte";
  import HomePane from "./lib/HomePane.svelte";
  import RecyclePane from "./lib/RecyclePane.svelte";
  import SettingsPane from "./lib/SettingsPane.svelte";
  import StatusBar from "./lib/StatusBar.svelte";
  import {
    app, tabs, refresh, back, forward, up, selectAll, hooks, firstSel, newTab, closeTab, switchTab,
    searchUpdate, loadPins, reopenClosedTab, onFsChanged, can, navigate, refreshClipState,
    THIS_PC, HOME, RECYCLE, SETTINGS, NETWORK, loadDrives, saveSession, restoreSession,
  } from "./lib/store.svelte";

  // Keep the record for "Reopen tabs from last time" current as tabs open,
  // move and close (it reads every tab's location and the active tab).
  $effect(saveSession);
  // Other windows offer this window's folder as a Copy to / Move to target.
  $effect(() => {
    void app.path;
    advertise();
  });
  import { moveFocus, toggleRow, typeToSelect, openSelection, focusedRow } from "./lib/selection.svelte";
  import { del, copySel, paste, newFolder, startRename, undo, redo, propertiesSel } from "./lib/ops.svelte";
  import BatchRename from "./lib/BatchRename.svelte";
  import Dialog from "./lib/Dialog.svelte";
  import ExtractDialog from "./lib/ExtractDialog.svelte";
  import Notice from "./lib/Notice.svelte";
  import { initDrag } from "./lib/drag";
  import { settings, setSetting, showPane } from "./lib/settings.svelte";
  import DetailsPane from "./lib/DetailsPane.svelte";
  import Splitter from "./lib/Splitter.svelte";
  import { initTabTransfers, advertise } from "./lib/tabtransfer";
  import ToolDialog from "./lib/ToolDialog.svelte";
  import ShellPane from "./lib/ShellPane.svelte";
  import { isShellLocation } from "./lib/shellplaces";

  let addressEdit: { focus: () => void; focusSearch: () => void } | undefined = $state();

  let viewportWidth = $state(innerWidth);
  // Leave enough room for the list even when both saved panes are wide.
  const navWidth = $derived(Math.min(settings.navWidth, Math.max(160, viewportWidth - 500)));
  const paneWidth = $derived(Math.min(settings.paneWidth, 600, Math.max(200, viewportWidth - navWidth - 285)));

  onMount(() => {
    let stopTransfers = () => {};
    const unlisten: Promise<() => void>[] = [
      listen<string>("fs-changed", (ev) => onFsChanged(ev.payload)),
      // Any app's copy or cut: Paste and the ghosted cut items follow at once.
      listen("clipboard-changed", () => refreshClipState()),
      // A USB drive plugged in or out, a disc, a network drive mapped.
      listen("drives-changed", () => { loadDrives(); if (isShellLocation(app.path)) refresh(); }),
      listen<Parameters<typeof searchUpdate>[0]>("search-update", (ev) => searchUpdate(ev.payload)),
    ];
    initDrag();
    (async () => {
      app.home = await invoke<string>("home_dir");
      loadPins();
      const transfers = await initTabTransfers();
      stopTransfers = transfers.destroy;
      if (transfers.imported) return;
      // `filezy <folder>` or Explorer's `filezy /select,<file>`; otherwise the
      // last window's tabs (when Reopen tabs is on and no other window is
      // open, so Ctrl+N doesn't copy them), else the start page.
      const launch = await invoke<{ path: string; select: string | null } | null>("launch_target").catch(() => null);
      if (!launch) {
        if (settings.restoreTabs && (await invoke<boolean>("first_window").catch(() => false)) && (await restoreSession())) return;
        return newTab();
      }
      await newTab(launch.path);
      if (launch.select) await navigate(launch.path, false, { refresh: true, select: [launch.select] });
    })();
    // Warm the shell-extension DLLs in the background so the first real
    // right-click doesn't pay the multi-second cold QueryContextMenu cost.
    // Pointless when extension entries are switched off in Settings.
    if (settings.shellExtras) {
      setTimeout(() => invoke("shell_menu_paths", { paths: [app.home] }).catch(() => {}), 2500);
    }
    loadDrives();
    invoke<typeof app.specials>("special_dirs").then((s) => (app.specials = s)).catch(() => {});
    invoke<typeof app.cloudRoots>("cloud_roots").then((r) => (app.cloudRoots = r)).catch(() => {});
    return () => { stopTransfers(); unlisten.forEach((u) => u.then((f) => f())); };
  });

  function isTextField(el: EventTarget | null): boolean {
    return (
      el instanceof HTMLInputElement ||
      el instanceof HTMLTextAreaElement ||
      el instanceof HTMLSelectElement ||
      (el instanceof HTMLElement && el.isContentEditable)
    );
  }

  /** File-list keys act when the list, or nothing in particular, has focus. */
  function listFocused(el: EventTarget | null): boolean {
    if (!(el instanceof HTMLElement) || el === document.body) return true;
    return !!el.closest("[data-filelist]");
  }

  function textSelectedOutsideList(): boolean {
    const s = getSelection();
    const at = s?.anchorNode instanceof Element ? s.anchorNode : s?.anchorNode?.parentElement;
    return !!s && !s.isCollapsed && !!at && !at.closest("[data-filelist]");
  }

  /// F6 / Shift+F6: move between the address bar, the folder tree and the
  /// content, like Explorer's pane cycling.
  function cyclePanes(dir: number) {
    const regions = [
      document.querySelector<HTMLElement>(".addressbox"),
      document.querySelector<HTMLElement>(".navpane [tabindex='0']"),
      document.querySelector<HTMLElement>("[data-filelist] .scroller") ??
        document.querySelector<HTMLElement>(".filecard [tabindex='0'], .filecard button, .filecard input"),
    ].filter((r): r is HTMLElement => !!r);
    const now = regions.findIndex((r) => r.contains(document.activeElement) || r === document.activeElement);
    const next = regions[(now + dir + regions.length) % regions.length];
    next?.focus();
  }

  function onKeydown(ev: KeyboardEvent) {
    const k = ev.key.length === 1 ? ev.key.toLowerCase() : ev.key;
    const ctrl = ev.ctrlKey && !ev.altKey;
    const typing = isTextField(ev.target);
    const handled = () => ev.preventDefault();

    // ---- window-wide: the most specific modifier combination first ----
    if (ctrl && ev.shiftKey && k === "t") return handled(), reopenClosedTab();
    if (ctrl && !ev.shiftKey && k === "t") return handled(), newTab();
    if (ctrl && !ev.shiftKey && k === "w") return handled(), closeTab(app.activeTab);
    if (ctrl && k === "Tab") {
      handled();
      const i = tabs.findIndex((t) => t.id === app.activeTab);
      const next = tabs[(i + (ev.shiftKey ? tabs.length - 1 : 1)) % tabs.length];
      if (next) switchTab(next.id);
      return;
    }
    if (ctrl && !ev.shiftKey && /^[1-9]$/.test(k)) {
      // Ctrl+1..8 picks that tab, Ctrl+9 the last one, as in Explorer and browsers.
      handled();
      const t = k === "9" ? tabs[tabs.length - 1] : tabs[+k - 1];
      if (t) switchTab(t.id);
      return;
    }
    if (ctrl && ev.shiftKey && k === "n") {
      if (typing) return;
      return handled(), newFolder();
    }
    if (ctrl && !ev.shiftKey && k === "n") return handled(), invoke("new_window").catch(() => {});
    if (ev.altKey && k === "ArrowLeft") return handled(), back();
    if (ev.altKey && k === "ArrowRight") return handled(), forward();
    if (ev.altKey && k === "ArrowUp") return handled(), up();
    if ((ctrl && k === "l") || (ev.altKey && !ev.ctrlKey && k === "d") || k === "F4") {
      return handled(), addressEdit?.focus();
    }
    if ((ctrl && (k === "f" || k === "e")) || k === "F3") return handled(), addressEdit?.focusSearch();
    if (k === "F5" || (ctrl && k === "r")) return handled(), refresh();
    if (k === "F6") return handled(), cyclePanes(ev.shiftKey ? -1 : 1);
    // Alt+P: Preview pane; Alt+Shift+P: Details pane (one at a time, as in Explorer).
    if (ev.altKey && !ev.ctrlKey && ev.shiftKey && k.toLowerCase() === "p") {
      return handled(), showPane(settings.detailsPane ? null : "details");
    }
    if (ev.altKey && !ev.ctrlKey && k === "p") return handled(), showPane(settings.previewPane ? null : "preview");
    if (ev.altKey && k === "Enter") {
      handled();
      if (firstSel() !== undefined) propertiesSel();
      return;
    }
    if (k === "Escape" && app.path === SETTINGS && !typing) return handled(), back();

    // ---- file list ----
    if (typing || !listFocused(ev.target)) return;
    // Text selected in a preview, the Details pane or a message: Ctrl+C and X
    // act on that text as in any page, not on the selected files.
    if (ctrl && (k === "c" || k === "x") && textSelectedOutsideList()) return;
    if (!can("select")) return;
    const mods = { shift: ev.shiftKey, ctrl: ev.ctrlKey };
    const cur = focusedRow() ?? -1;
    if (k === "Backspace") return handled(), back();
    if (ctrl && !ev.shiftKey && k === "a") return handled(), selectAll();
    if (ctrl && k === "c") return handled(), copySel(false);
    if (ctrl && k === "x") return handled(), copySel(true);
    if (ctrl && k === "v") return handled(), paste();
    if (ctrl && !ev.shiftKey && k === "z") return handled(), undo();
    if ((ctrl && k === "y") || (ctrl && ev.shiftKey && k === "z")) return handled(), redo();
    if (k === "F2") return handled(), startRename();
    // Shift+Delete deletes for good, after Windows' own confirmation.
    if (k === "Delete") return handled(), del(ev.shiftKey);
    if (k === "Enter") return handled(), openSelection();
    if ((ev.shiftKey && k === "F10") || k === "ContextMenu") return handled(), hooks.contextMenu?.();
    if (k === " " && ev.ctrlKey) {
      handled();
      if (cur >= 0) toggleRow(cur);
      return;
    }
    if (!app.total) return;
    const vertical = k === "ArrowDown" || k === "ArrowUp";
    if (vertical || k === "ArrowLeft" || k === "ArrowRight") {
      handled();
      // The first arrow press puts the keyboard on the first item.
      if (cur < 0) return moveFocus(0, mods);
      const step = vertical ? (hooks.gridCols ?? 1) : (hooks.hStep ?? 0);
      if (!step) return;
      const earlier = k === "ArrowUp" || k === "ArrowLeft";
      return moveFocus(cur + (earlier ? -step : step), mods);
    }
    if (k === "Home") return handled(), moveFocus(0, mods);
    if (k === "End") return handled(), moveFocus(app.total - 1, mods);
    if (k === "PageDown" || k === "PageUp") {
      handled();
      const page = hooks.pageSize ?? 10;
      return moveFocus(Math.max(cur, 0) + (k === "PageUp" ? -page : page), mods);
    }
    if (k === " " && !ev.shiftKey && cur >= 0) return handled(), moveFocus(cur);
    // Typing a name jumps to it.
    if (ev.key.length === 1 && !ev.ctrlKey && !ev.altKey && ev.key !== " ") {
      handled();
      typeToSelect(ev.key);
    }
  }
</script>

<!-- Focus re-reads the clipboard too, in case a change event was missed. -->
<svelte:window onkeydown={onKeydown} onfocus={refreshClipState} bind:innerWidth={viewportWidth} />

<div class="root" style={`--nav-width:${navWidth}px`}>
  <TitleBar />
  <AddressBar bind:this={addressEdit} />
  <CommandBar />
  <div class="content">
    <NavPane />
    <div class="navgrip">
      <Splitter label={msg("Navigation pane width")} value={navWidth} min={160} max={Math.min(400, Math.max(160, viewportWidth - 500))} resize={(w) => setSetting("navWidth", w)} />
    </div>
    <div class="filecard">
      {#if app.path === THIS_PC}
        <DrivesPane />
      {:else if app.path === NETWORK}
        <NetworkPane />
      {:else if app.path === HOME}
        <HomePane />
      {:else if app.path === RECYCLE}
        <RecyclePane />
      {:else if app.path === SETTINGS}
        <SettingsPane />
      {:else if isShellLocation(app.path)}
        <ShellPane />
      {:else}
        <div class="filerow">
          <FilePane />
          {#if settings.previewPane || settings.detailsPane}
            <div class="sidepane" style="width:{paneWidth}px">
              <div class="panegrip"><Splitter label={msg("Pane width")} value={paneWidth} min={200} max={600} reverse resize={(w) => setSetting("paneWidth", w)} /></div>
              {#if settings.detailsPane}
                <DetailsPane />
              {:else}
                <PreviewPane />
              {/if}
            </div>
          {/if}
        </div>
        <StatusBar />
      {/if}
      <Notice />
    </div>
  </div>
</div>
<Dialog />
<ExtractDialog />
<BatchRename />
<ToolDialog />

<style>
  .root {
    height: 100vh;
    display: flex;
    flex-direction: column;
    /* An opaque base also keeps covered-window captures readable. */
    background: var(--bg, var(--mica-fallback));
  }
  .content {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .navgrip { display: flex; }
  @media (max-width: 700px) { .navgrip { display: none; } }
  .filerow {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .sidepane {
    position: relative;
    flex-shrink: 0;
    border-left: 1px solid var(--stroke);
    overflow: hidden;
  }
  .panegrip {
    position: absolute;
    left: -3px;
    top: 0;
    bottom: 0;
    width: 5px;
    display: flex;
    cursor: col-resize;
    z-index: 1;
  }
  @media (max-width: 900px) {
    .sidepane {
      display: none;
    }
  }
  .filecard {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background: var(--card);
    border: 1px solid var(--stroke);
    border-right: none;
    border-bottom: none;
    border-top-left-radius: 8px;
  }
</style>
