<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { SvelteMap } from "svelte/reactivity";
  import { untrack } from "svelte";
  import {
    app, navigate, newTab, openPath, recentFiles, fmtDate, folderName, shellIcon, pinRemove, movePin,
    isFavorite, setFavorite, removeRecent, type ExtMeta,
  } from "./store.svelte";
  import { openFileLocation } from "./ops.svelte";
  import { menuKeys, fitMenu } from "./menukeys";
  import { menuOn } from "./settings.svelte";
  import { baseName, isWithin, parentPath, rootOf, samePath } from "./paths";

  const c = String.fromCharCode;
  const glyphs: Record<string, string> = {
    Desktop: c(0xe7f4),
    Documents: c(0xe8a5),
    Downloads: c(0xe896),
    Pictures: c(0xeb9f),
    Music: c(0xe8d6),
    Videos: c(0xe714),
  };

  const extOf = (p: string) => (/\.([^.\\]+)$/.exec(p)?.[1] ?? "").toLowerCase();
  const favorites = $derived(app.favorites.map((p) => ({ path: p, name: baseName(p), ext: extOf(p) })));

  // Per-extension icons for the file lists, fetched once each (outside
  // rendering, which mustn't change state).
  const icons = new SvelteMap<string, ExtMeta>();
  $effect(() => {
    for (const ext of new Set([...favorites, ...recentFiles.list].map((f) => f.ext))) {
      if (untrack(() => icons.has(ext))) continue;
      icons.set(ext, { icon: null, type_name: "" });
      invoke<ExtMeta>("ext_meta", { ext, isDir: false }).then((m) => icons.set(ext, m));
    }
  });
  const iconFor = (ext: string) => icons.get(ext)?.icon ?? null;

  /** Where an item lives, as Explorer's Home words it: the cloud folder's
   *  name for anything in one, "Stored locally" for the standard folders,
   *  else the folder holding it ("This PC" for a drive). */
  function where(p: string, special = false): string {
    const cloud = app.cloudRoots.find((r) => isWithin(p, r.path));
    if (cloud) return cloud.name;
    if (special) return "Stored locally";
    if (samePath(p, rootOf(p) ?? "")) return "This PC";
    return parentPath(p) ?? "";
  }

  // ---- the item menu ----
  type Menu = { x: number; y: number; path: string; folder: boolean; pin: boolean; recent: boolean };
  let menu = $state<Menu | null>(null);
  function openMenu(ev: MouseEvent, path: string, kind: { folder?: boolean; pin?: boolean; recent?: boolean }) {
    ev.preventDefault();
    menu = { x: ev.clientX, y: ev.clientY, path, folder: !!kind.folder, pin: !!kind.pin, recent: !!kind.recent };
  }
  /// Close the menu and act on what it was opened for. The action gets that as
  /// a plain value: the template's {@const}s read `menu`, which is null by then.
  function run(f: (m: Menu) => unknown) {
    const m = menu!;
    menu = null;
    Promise.resolve(f(m)).catch((e) => (app.status = String(e)));
  }

  // ---- pins are dragged to reorder them, as in Explorer's Quick access ----
  let drag = $state<{ from: number; x: number; y: number; moved: boolean } | null>(null);
  function pinDown(ev: MouseEvent, i: number) {
    if (ev.button === 0) drag = { from: i, x: ev.clientX, y: ev.clientY, moved: false };
  }
  function pinMove(ev: MouseEvent) {
    if (!drag) return;
    if (!drag.moved && Math.abs(ev.clientX - drag.x) + Math.abs(ev.clientY - drag.y) < 6) return;
    drag.moved = true;
    const over = [...document.querySelectorAll<HTMLElement>(".homepane [data-pin]")].find((el) => {
      const r = el.getBoundingClientRect();
      return ev.clientX >= r.left && ev.clientX <= r.right && ev.clientY >= r.top && ev.clientY <= r.bottom;
    });
    const to = over ? +over.dataset.pin! : -1;
    if (to >= 0 && to !== drag.from) {
      movePin(drag.from, to);
      drag.from = to;
    }
  }
</script>

<svelte:window
  onmousemove={pinMove}
  onmouseup={() => (drag = null)}
  onmousedown={(e) => { if (menu && !(e.target as HTMLElement).closest(".menu")) menu = null; }}
/>

{#if menu}
  {@const m = menu}
  <div class="menu" style="left:{m.x}px; top:{m.y}px" use:menuKeys={() => (menu = null)} use:fitMenu={m}>
    {#if menuOn("home.open")}
      <button class="menu-item" onclick={() => run((x) => (x.folder ? navigate(x.path) : openPath(x.path)))}>
        <span class="glyph">&#xE8E5;</span> Open
      </button>
    {/if}
    {#if m.folder && menuOn("home.newtab")}
      <button class="menu-item" onclick={() => run((x) => newTab(x.path))}>
        <span class="glyph">&#xE8A7;</span> Open in new tab
      </button>
    {/if}
    {#if m.folder && menuOn("home.newwindow")}
      <button class="menu-item" onclick={() => run((x) => invoke("new_window", { path: x.path }))}>
        <span class="glyph">&#xE737;</span> Open in new window
      </button>
    {/if}
    {#if !m.folder && menuOn("home.openlocation")}
      <button class="menu-item" onclick={() => run((x) => openFileLocation(x.path))}>
        <span class="glyph">&#xE838;</span> Open file location
      </button>
    {/if}
    {#if m.pin && menuOn("home.pin")}
      <button class="menu-item" onclick={() => run((x) => pinRemove(x.path))}>
        <span class="glyph">&#xE77A;</span> Unpin from Quick access
      </button>
    {/if}
    {#if !m.folder && menuOn("home.favorite")}
      {@const fav = isFavorite(m.path)}
      <button class="menu-item" onclick={() => run((x) => setFavorite([x.path], !isFavorite(x.path)))}>
        <span class="glyph">{fav ? "" : ""}</span> {fav ? "Remove from Favorites" : "Add to Favorites"}
      </button>
    {/if}
    {#if m.recent && menuOn("home.removerecent")}
      <button class="menu-item" onclick={() => run((x) => removeRecent([x.path]))}>
        <span class="glyph">&#xE711;</span> Remove from Recent
      </button>
    {/if}
    {#if menuOn("home.properties")}
      <div class="menu-sep"></div>
      <button class="menu-item" onclick={() => run((x) => invoke("show_properties", { path: x.path }))}>
        <span class="glyph">&#xE90F;</span> Properties
      </button>
    {/if}
  </div>
{/if}

<div class="homepane">
  <div class="section">Quick access</div>
  <div class="qtiles">
    {#each app.specials as [name, path] (path)}
      <button
        class="qtile"
        ondblclick={() => navigate(path)}
        oncontextmenu={(e) => openMenu(e, path, { folder: true })}
        title={path}
        aria-label={path}
        data-drop={path}
        data-drop-label={name}
      >
        {#if shellIcon(path, 48)}
          <img class="qimg" src={shellIcon(path, 48)} alt="" width="40" height="40" />
        {:else}
          <span class="glyph qicon">{glyphs[name] ?? c(0xe8b7)}</span>
        {/if}
        <span class="qinfo">
          <span class="qname">{name}</span>
          <span class="qsub">{where(path, true)}</span>
        </span>
        <span class="glyph pin">&#xE840;</span>
      </button>
    {/each}
    {#each app.pins as p, i (p)}
      <button
        class="qtile"
        class:dragging={drag?.moved && drag.from === i}
        ondblclick={() => navigate(p)}
        onmousedown={(e) => pinDown(e, i)}
        oncontextmenu={(e) => openMenu(e, p, { folder: true, pin: true })}
        title={p}
        aria-label={p}
        data-pin={i}
        data-drop={p}
        data-drop-label={folderName(p)}
      >
        {#if shellIcon(p, 48)}
          <img class="qimg" src={shellIcon(p, 48)} alt="" width="40" height="40" />
        {:else}
          <span class="glyph qicon">{c(0xe8b7)}</span>
        {/if}
        <span class="qinfo">
          <span class="qname">{folderName(p)}</span>
          <span class="qsub">{where(p)}</span>
        </span>
        <span class="glyph pin">&#xE840;</span>
      </button>
    {/each}
  </div>

  <div class="section">Favorites</div>
  {#if !favorites.length}
    <div class="empty">Files you add to Favorites show up here.</div>
  {:else}
    <div class="recent" aria-label="Favorites">
      {#each favorites as f (f.path)}
        <button class="rrow" ondblclick={() => openPath(f.path)} oncontextmenu={(e) => openMenu(e, f.path, {})} title={f.path} aria-label={f.path}>
          {#if iconFor(f.ext)}
            <img class="ricon" src={iconFor(f.ext)} alt="" width="16" height="16" />
          {:else}
            <span class="glyph ricon">&#xE7C3;</span>
          {/if}
          <span class="rname">{f.name}</span>
          <span class="rpath">{where(f.path)}</span>
        </button>
      {/each}
    </div>
  {/if}

  <div class="section">Recent files</div>
  {#if !recentFiles.list.length}
    <div class="empty">After you open some files, they show up here.</div>
  {:else}
    <div class="recent" aria-label="Recent files">
      {#each recentFiles.list as f (f.path)}
        <button class="rrow" ondblclick={() => openPath(f.path)} oncontextmenu={(e) => openMenu(e, f.path, { recent: true })} title={f.path} aria-label={f.path}>
          {#if iconFor(f.ext)}
            <img class="ricon" src={iconFor(f.ext)} alt="" width="16" height="16" />
          {:else}
            <span class="glyph ricon">&#xE7C3;</span>
          {/if}
          <span class="rname">{f.name}</span>
          <span class="rpath">{where(f.path)}</span>
          <span class="rdate">{fmtDate(f.modified)}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .homepane {
    flex: 1;
    overflow-y: auto;
    padding: 12px 16px;
  }
  .section {
    font-size: 13px;
    color: var(--text-2);
    padding: 6px 0 10px 4px;
  }
  .qtiles {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-bottom: 14px;
  }
  .qtile {
    width: 216px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 4px;
    text-align: left;
  }
  .qtile:hover {
    background: var(--hover);
  }
  .qtile.dragging {
    background: var(--sel);
  }
  .qicon {
    font-size: 28px;
    color: var(--text-2);
    flex-shrink: 0;
  }
  .qimg {
    flex-shrink: 0;
  }
  .qinfo {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .qname,
  .qsub {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .qname {
    font-size: 13px;
  }
  .qsub {
    font-size: 11px;
    color: var(--text-3);
  }
  .pin {
    font-size: 11px;
    color: var(--text-3);
    transform: rotate(45deg);
  }
  .empty {
    font-size: 13px;
    color: var(--text-3);
    padding: 8px 4px;
    margin-bottom: 14px;
  }
  .recent {
    display: flex;
    flex-direction: column;
    margin-bottom: 14px;
  }
  .rrow {
    height: 34px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 10px;
    border-radius: 4px;
    text-align: left;
  }
  .rrow:hover {
    background: var(--hover);
  }
  .ricon {
    flex-shrink: 0;
    font-size: 15px;
    color: var(--text-2);
  }
  .rname {
    flex: 0 1 auto;
    min-width: 0;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rpath {
    flex: 1;
    min-width: 0;
    font-size: 12px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-left: 12px;
  }
  .rdate {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-3);
  }
</style>
