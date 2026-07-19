<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import TitleBar from "./lib/TitleBar.svelte";
  import AddressBar from "./lib/AddressBar.svelte";
  import CommandBar from "./lib/CommandBar.svelte";
  import NavPane from "./lib/NavPane.svelte";
  import FilePane from "./lib/FilePane.svelte";
  import DrivesPane from "./lib/DrivesPane.svelte";
  import PreviewPane from "./lib/PreviewPane.svelte";
  import HomePane from "./lib/HomePane.svelte";
  import RecyclePane from "./lib/RecyclePane.svelte";
  import StatusBar from "./lib/StatusBar.svelte";
  import {
    app, sel, tabs, navigate, refresh, back, forward, up, del, copySel, paste,
    newFolder, selectAll, startRename, openEntry, hooks, selCount, selIndices, clearSel,
    newTab, closeTab, switchTab, searchUpdate, loadPins, THIS_PC, HOME, RECYCLE,
  } from "./lib/store.svelte";

  let addressEdit: { focus: () => void } | undefined = $state();

  onMount(async () => {
    app.home = await invoke<string>("home_dir");
    loadPins();
    newTab(); // lands on the Home page, like Explorer
    // Warm the shell-extension DLLs in the background so the first real
    // right-click doesn't pay the multi-second cold QueryContextMenu cost.
    setTimeout(() => invoke("shell_menu", { dir: app.home, names: [] }).catch(() => {}), 2500);
    invoke<typeof app.drives>("drives").then((d) => (app.drives = d));
    invoke<typeof app.specials>("special_dirs").then((s) => (app.specials = s));
    invoke<string | null>("onedrive_dir").then((p) => (app.onedrive = p ?? ""));
    const un1 = await listen<string>("fs-changed", (ev) => {
      if (ev.payload === app.path) refresh();
    });
    const un2 = await listen<{ total: number; done: boolean }>("search-update", (ev) => {
      searchUpdate(ev.payload.total, ev.payload.done);
    });
    return () => {
      un1();
      un2();
    };
  });

  function onKeydown(ev: KeyboardEvent) {
    if (ev.target instanceof HTMLInputElement) return;
    const k = ev.key;
    if (ev.ctrlKey && (k === "t" || k === "T")) { newTab(); ev.preventDefault(); return; }
    if (ev.ctrlKey && (k === "w" || k === "W")) { closeTab(app.activeTab); ev.preventDefault(); return; }
    if (ev.ctrlKey && k === "Tab") {
      const i = tabs.findIndex((t) => t.id === app.activeTab);
      const next = tabs[(i + (ev.shiftKey ? tabs.length - 1 : 1)) % tabs.length];
      if (next) switchTab(next.id);
      ev.preventDefault();
      return;
    }
    if (ev.altKey && (k === "p" || k === "P")) { app.previewPane = !app.previewPane; ev.preventDefault(); return; }
    if (ev.altKey && k === "Enter") {
      const first = selIndices()[0];
      if (first !== undefined) invoke("show_properties", { index: first }).catch(() => {});
      ev.preventDefault();
      return;
    }
    if (ev.altKey && k === "ArrowLeft") { back(); ev.preventDefault(); }
    else if (ev.altKey && k === "ArrowRight") { forward(); ev.preventDefault(); }
    else if ((ev.altKey && k === "ArrowUp") || k === "Backspace") { up(); ev.preventDefault(); }
    else if (ev.ctrlKey && ev.shiftKey && (k === "N" || k === "n")) { newFolder(); ev.preventDefault(); }
    else if (ev.ctrlKey && (k === "a" || k === "A")) { selectAll(); ev.preventDefault(); }
    else if (ev.ctrlKey && (k === "c" || k === "C")) copySel(false);
    else if (ev.ctrlKey && (k === "x" || k === "X")) copySel(true);
    else if (ev.ctrlKey && (k === "v" || k === "V")) paste();
    else if (ev.ctrlKey && (k === "l" || k === "L")) { addressEdit?.focus(); ev.preventDefault(); }
    else if (k === "F5") { refresh(); ev.preventDefault(); }
    else if (k === "F2") startRename();
    else if (k === "Delete") del();
    else if (k === "Enter" && selCount()) openEntry(selIndices()[0]);
    else if (k === "ArrowDown" || k === "ArrowUp" || k === "ArrowLeft" || k === "ArrowRight") {
      ev.preventDefault();
      if (!app.total) return;
      const step = k === "ArrowDown" || k === "ArrowUp" ? (hooks.gridCols ?? 1) : (hooks.hStep ?? 1);
      const fwd = k === "ArrowDown" || k === "ArrowRight";
      // Iterate rather than spread: Math.max(...set) throws RangeError on huge selections.
      let cur = app.allSelected ? app.total - 1 : -1;
      let lo = app.allSelected ? 0 : app.total;
      for (const idx of sel) {
        if (idx > cur) cur = idx;
        if (idx < lo) lo = idx;
      }
      let next = fwd ? Math.min(app.total - 1, cur + step) : Math.max(0, lo - step);
      if (ev.shiftKey && sel.size) {
        sel.add(next);
      } else {
        clearSel();
        sel.add(next);
        app.anchor = next;
      }
      hooks.scrollToIndex?.(next);
    } else if (k === "Home" && app.total) {
      clearSel(); sel.add(0); app.anchor = 0; hooks.scrollToIndex?.(0);
    } else if (k === "End" && app.total) {
      const last = app.total - 1;
      clearSel(); sel.add(last); app.anchor = last; hooks.scrollToIndex?.(last);
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="root">
  <TitleBar />
  <AddressBar bind:this={addressEdit} />
  <CommandBar />
  <div class="content">
    <NavPane />
    <div class="filecard">
      {#if app.path === THIS_PC}
        <DrivesPane />
      {:else if app.path === HOME}
        <HomePane />
      {:else if app.path === RECYCLE}
        <RecyclePane />
      {:else}
        <div class="filerow">
          <FilePane />
          {#if app.previewPane}
            <PreviewPane />
          {/if}
        </div>
        <StatusBar />
      {/if}
    </div>
  </div>
</div>

<style>
  .root {
    height: 100vh;
    display: flex;
    flex-direction: column;
    /* Mica themes stay transparent; community palettes paint their own base. */
    background: var(--bg, transparent);
  }
  .content {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .filerow {
    flex: 1;
    min-height: 0;
    display: flex;
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
