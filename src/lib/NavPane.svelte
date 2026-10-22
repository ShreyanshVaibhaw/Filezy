<script lang="ts">
  import { msg } from "./i18n.svelte";
  import { menuKeys } from "./menukeys";
  import { app, navigate, folderName, pinRemove, movePin, shellIcon, treeDirty, THIS_PC, HOME, RECYCLE, RECYCLE_GUID, NETWORK } from "./store.svelte";
  import { menuOn, settings } from "./settings.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";
  import { dropHover } from "./drag";
  import { DEVICES } from "./shellplaces";

  let wslRoots = $state<[string, string][]>([]);
  invoke<[string, string][]>("wsl_roots").then((roots) => wslRoots = roots).catch(() => {});

  let pcOpen = $state(true);

  // Lazy folder tree: path -> children names (null = loading).
  const kids = new SvelteMap<string, string[] | null>();
  const open = new SvelteSet<string>();

  function joinDir(parent: string, name: string): string {
    return parent.replace(/\\$/, "") + "\\" + name;
  }

  function load(path: string) {
    if (!kids.has(path)) kids.set(path, null);
    invoke<string[]>("list_dirs", { path, showHidden: settings.showHidden, showProtected: settings.showProtected })
      .then((d) => kids.set(path, d))
      .catch(() => kids.set(path, []));
  }

  // Expanding always re-reads (showing what was known meanwhile), like Explorer.
  function toggle(path: string, ev?: MouseEvent) {
    ev?.stopPropagation();
    if (open.has(path)) {
      open.delete(path);
      return;
    }
    open.add(path);
    load(path);
  }

  // A folder's contents changed: re-read it if the tree shows it, forget it otherwise.
  $effect(() => {
    void treeDirty.n;
    const p = treeDirty.path.replace(/\\+$/, "").toLowerCase();
    for (const k of [...kids.keys()]) {
      if (k.replace(/\\+$/, "").toLowerCase() !== p) continue;
      if (open.has(k)) load(k);
      else kids.delete(k);
    }
  });

  // Hidden-item settings change which folders the tree shows.
  $effect(() => {
    void settings.showHidden;
    void settings.showProtected;
    for (const k of [...kids.keys()]) {
      if (open.has(k)) load(k);
      else kids.delete(k);
    }
  });

  const specialGlyphs: Record<string, string> = {
    Desktop: "",
    Documents: "",
    Downloads: "",
    Pictures: "",
    Music: "",
    Videos: "",
  };

  function isActive(p: string): boolean {
    return app.path.replace(/\\$/, "").toLowerCase() === p.replace(/\\$/, "").toLowerCase();
  }

  const folderGlyph = String.fromCharCode(0xe8b7);

  // ---- keyboard: one Tab stop for the whole tree, arrows inside it ----
  // The item that takes Tab focus (roving tabindex); it follows the keyboard.
  let focusKey = $state(HOME);
  const tabTo = (key: string) => (key === focusKey ? 0 : -1);

  function toggleKey(key: string) {
    if (key === THIS_PC) pcOpen = !pcOpen;
    else toggle(key);
  }

  function treeKeys(e: KeyboardEvent) {
    const items = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>(".navitem")];
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    if (i < 0) return;
    const it = items[i];
    const level = +(it.dataset.level ?? 1);
    const expanded = it.getAttribute("aria-expanded");
    const go = (j: number) => items[Math.max(0, Math.min(items.length - 1, j))]?.focus();
    switch (e.key) {
      case "ArrowDown":
        go(i + 1);
        break;
      case "ArrowUp":
        go(i - 1);
        break;
      case "Home":
        go(0);
        break;
      case "End":
        go(items.length - 1);
        break;
      case "ArrowRight":
        if (expanded === "false") toggleKey(it.dataset.key!);
        else if (expanded === "true") go(i + 1);
        break;
      case "ArrowLeft":
        if (expanded === "true") toggleKey(it.dataset.key!);
        else for (let j = i - 1; j >= 0; j--) if (+(items[j].dataset.level ?? 1) < level) return void (go(j), e.preventDefault());
        break;
      default: {
        if (e.key.length !== 1 || e.ctrlKey || e.altKey) return;
        const k = e.key.toLowerCase();
        for (let s = 1; s <= items.length; s++) {
          const b = items[(i + s) % items.length];
          if (b.querySelector(".label")?.textContent?.trim().toLowerCase().startsWith(k)) {
            b.focus();
            break;
          }
        }
      }
    }
    e.preventDefault();
    e.stopPropagation();
  }
  let pinCtx = $state<{ x: number; y: number; path: string } | null>(null);
  function onPinContext(ev: MouseEvent, path: string) {
    ev.preventDefault();
    pinCtx = { x: ev.clientX, y: ev.clientY, path };
  }

  // Pins are dragged up and down to reorder them, as in Explorer. A drag that
  // moved swallows the click its mouse-up makes, so it doesn't open the pin.
  let pinDrag = $state<{ from: number; y: number; moved: boolean } | null>(null);
  let swallowClick = false;
  function pinDown(ev: MouseEvent, i: number) {
    swallowClick = false;
    if (ev.button === 0) pinDrag = { from: i, y: ev.clientY, moved: false };
  }
  function pinUp() {
    if (pinDrag?.moved) swallowClick = true;
    pinDrag = null;
  }
  function pinClick(p: string) {
    if (swallowClick) swallowClick = false;
    else navigate(p);
  }
  function pinMove(ev: MouseEvent) {
    if (!pinDrag) return;
    if (!pinDrag.moved && Math.abs(ev.clientY - pinDrag.y) < 5) return;
    pinDrag.moved = true;
    const over = [...document.querySelectorAll<HTMLElement>(".navpane [data-pin]")].find((el) => {
      const r = el.getBoundingClientRect();
      return ev.clientY >= r.top && ev.clientY <= r.bottom;
    });
    const to = over ? +over.dataset.pin! : -1;
    if (to >= 0 && to !== pinDrag.from) {
      movePin(pinDrag.from, to);
      pinDrag.from = to;
    }
  }
</script>

{#snippet treeNode(path: string, label: string, glyph: string, depth: number)}
  <button
    class="navitem"
    class:active={isActive(path)}
    style="padding-left: {6 + depth * 16}px"
    onclick={() => navigate(path)}
    title={path}
    role="treeitem"
    data-key={path}
    data-drop={path}
    data-drop-label={label}
    use:dropHover={() => !open.has(path) && toggle(path)}
    data-level={depth + 1}
    aria-level={depth + 1}
    aria-expanded={open.has(path)}
    aria-selected={isActive(path)}
    tabindex={tabTo(path)}
  >
    <span
      class="glyph chevslot chev"
      role="none"
      onmousedown={(ev) => toggle(path, ev)}
      onclick={(ev) => ev.stopPropagation()}
    >
      {open.has(path) ? "" : ""}
    </span>
    {#if shellIcon(path, 48)}
      <img class="navicon" src={shellIcon(path, 48)} alt="" width="16" height="16" />
    {:else}
      <span class="glyph icon">{glyph}</span>
    {/if}
    <span class="label">{label}</span>
  </button>
  {#if open.has(path)}
    {#if kids.get(path) === null}
      <div class="loading" style="padding-left: {28 + (depth + 1) * 16}px">…</div>
    {:else}
      {#each kids.get(path) ?? [] as child (child)}
        {@render treeNode(joinDir(path, child), child, "", depth + 1)}
      {/each}
    {/if}
  {/if}
{/snippet}

<div
  class="navpane"
  role="tree"
  aria-label={msg("Navigation pane")}
  data-drop-scroll
  tabindex="-1"
  onkeydown={treeKeys}
  onfocusin={(e) => {
    const key = (e.target as HTMLElement).closest<HTMLElement>(".navitem")?.dataset.key;
    if (key) focusKey = key;
  }}
>
  <button
    class="navitem"
    class:active={app.path === HOME}
    onclick={() => navigate(HOME)}
    role="treeitem"
    data-key={HOME}
    data-level="1"
    aria-level={1}
    aria-selected={app.path === HOME}
    tabindex={tabTo(HOME)}
  >
    <span class="chevslot"></span>
    <span class="glyph icon">&#xE80F;</span>
    <span class="label">{msg("Home")}</span>
  </button>

  <div class="sep"></div>
  <button class="navitem" class:active={app.path === DEVICES} onclick={() => navigate(DEVICES)} role="treeitem" data-key={DEVICES} data-level="1" aria-level={1} aria-selected={app.path === DEVICES} tabindex={tabTo(DEVICES)}>
    <span class="chevslot"></span><span class="glyph icon">&#xE8EA;</span><span class="label">{msg("Devices")}</span>
  </button>
  {#each wslRoots as [name, path] (path)}
    <button class="navitem" class:active={app.path === path} onclick={() => navigate(path)} role="treeitem" data-key={path} data-level="1" aria-level={1} aria-selected={app.path === path} tabindex={tabTo(path)} title={msg("Open {name} in WSL. This starts the distribution if it is stopped.", { name })}>
      <span class="chevslot"></span><span class="glyph icon">&#xE756;</span><span class="label">{name}</span>
    </button>
  {/each}

  {#each app.specials as [name, path] (path)}
    <button
      class="navitem"
      class:active={isActive(path)}
      onclick={() => navigate(path)}
      role="treeitem"
      data-key={path}
      data-drop={path}
      data-drop-label={name}
      data-level="1"
      aria-level={1}
      aria-selected={isActive(path)}
      tabindex={tabTo(path)}
    >
      <span class="chevslot"></span>
      {#if shellIcon(path, 48)}
        <img class="navicon" src={shellIcon(path, 48)} alt="" width="16" height="16" />
      {:else}
        <span class="glyph icon">{specialGlyphs[name] ?? ""}</span>
      {/if}
      <span class="label">{name}</span>
      <span class="glyph pin">&#xE840;</span>
    </button>
  {/each}

  {#each app.pins as p, i (p)}
    <button
      class="navitem"
      class:active={isActive(p)}
      class:dragging={pinDrag?.moved && pinDrag.from === i}
      onclick={() => pinClick(p)}
      onmousedown={(ev) => pinDown(ev, i)}
      oncontextmenu={(ev) => onPinContext(ev, p)}
      data-pin={i}
      title={p}
      role="treeitem"
      data-key={p}
      data-drop={p}
      data-drop-label={folderName(p)}
      data-level="1"
      aria-level={1}
      aria-selected={isActive(p)}
      tabindex={tabTo(p)}
    >
      <span class="chevslot"></span>
      {#if shellIcon(p, 48)}
        <img class="navicon" src={shellIcon(p, 48)} alt="" width="16" height="16" />
      {:else}
        <span class="glyph icon">&#xE8B7;</span>
      {/if}
      <span class="label">{folderName(p)}</span>
      <span class="glyph pin">&#xE840;</span>
    </button>
  {/each}

  {#if app.cloudRoots.length}
    <div class="sep"></div>
    {#each app.cloudRoots as r (r.path)}
      {@render treeNode(r.path, r.name, folderGlyph, 0)}
    {/each}
  {/if}

  <div class="sep"></div>

  <button
    class="navitem"
    class:active={app.path === THIS_PC}
    onclick={() => navigate(THIS_PC)}
    role="treeitem"
    data-key={THIS_PC}
    use:dropHover={() => (pcOpen = true)}
    data-level="1"
    aria-level={1}
    aria-expanded={pcOpen}
    aria-selected={app.path === THIS_PC}
    tabindex={tabTo(THIS_PC)}
  >
    <span
      class="glyph chevslot chev"
      role="none"
      onmousedown={(ev) => { ev.stopPropagation(); pcOpen = !pcOpen; }}
      onclick={(ev) => ev.stopPropagation()}
    >
      {pcOpen ? "" : ""}
    </span>
    <span class="glyph icon">&#xE977;</span>
    <span class="label">{msg("This PC")}</span>
  </button>
  {#if pcOpen}
    {#each app.drives as d (d.path)}
      {@render treeNode(d.path, d.name, "", 1)}
    {/each}
  {/if}

  <button
    class="navitem"
    class:active={app.path === NETWORK || app.path.startsWith("\\\\")}
    onclick={() => navigate(NETWORK)}
    role="treeitem"
    data-key={NETWORK}
    data-level="1"
    aria-level={1}
    aria-selected={app.path === NETWORK}
    tabindex={tabTo(NETWORK)}
  >
    <span class="glyph chevslot"></span>
    <span class="glyph icon">&#xE968;</span>
    <span class="label">{msg("Network")}</span>
  </button>

  <div class="sep"></div>

  <button
    class="navitem"
    class:active={app.path === RECYCLE}
    onclick={() => navigate(RECYCLE)}
    role="treeitem"
    data-key={RECYCLE}
    data-drop={RECYCLE}
    data-drop-label="Recycle Bin"
    data-level="1"
    aria-level={1}
    aria-selected={app.path === RECYCLE}
    tabindex={tabTo(RECYCLE)}
  >
    <span class="chevslot"></span>
    {#if shellIcon(RECYCLE_GUID, 48)}
      <img class="navicon" src={shellIcon(RECYCLE_GUID, 48)} alt="" width="16" height="16" />
    {:else}
      <span class="glyph icon">&#xE74D;</span>
    {/if}
    <span class="label">{msg("Recycle Bin")}</span>
  </button>
</div>

{#if pinCtx && menuOn("nav.unpin")}
  <div class="menu" style="left:{pinCtx.x}px; top:{pinCtx.y}px" use:menuKeys={() => (pinCtx = null)}>
    <button
      class="menu-item"
      onclick={() => { pinRemove(pinCtx!.path); pinCtx = null; }}
    >
      <span class="glyph">&#xE77A;</span> {msg("Unpin from Quick access")}
    </button>
  </div>
{/if}

<svelte:window
  onmousedown={(e) => { if (pinCtx && !(e.target as HTMLElement).closest(".menu")) pinCtx = null; }}
  onmousemove={pinMove}
  onmouseup={pinUp}
/>

<style>
  .navpane {
    width: var(--nav-width, 240px);
    flex-shrink: 0;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 4px 6px 12px 4px;
  }
  .navitem {
    width: 100%;
    height: 32px;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 6px;
    margin-bottom: 1px;
    font-size: 13px;
  }
  .navitem:hover {
    background: var(--hover);
  }
  .navitem.active,
  .navitem.dragging {
    background: var(--sel);
  }
  .navitem:focus-visible {
    outline: 1px solid var(--text-2);
    outline-offset: -1px;
  }
  .navpane:focus {
    outline: none;
  }
  .navitem .chevslot {
    width: 16px;
    flex-shrink: 0;
    font-size: 9px;
    color: var(--text-3);
    text-align: center;
  }
  .navitem .chev:hover {
    color: var(--text);
  }
  .navitem .navicon {
    margin-right: 8px;
    flex-shrink: 0;
  }
  .navitem .icon {
    font-size: 16px;
    color: var(--text-2);
    margin-right: 8px;
    flex-shrink: 0;
  }
  .navitem .label {
    flex: 1;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .navitem .pin {
    font-size: 11px;
    color: var(--text-3);
    transform: rotate(45deg);
  }
  .loading {
    font-size: 12px;
    color: var(--text-3);
    padding: 2px 0;
  }
  .sep {
    height: 1px;
    background: var(--stroke);
    margin: 6px 8px;
  }
  @media (max-width: 700px) {
    .navpane {
      display: none;
    }
  }
  @media (forced-colors: active) {
    .navitem.active {
      forced-color-adjust: none;
      background: Highlight;
      color: HighlightText;
    }
    .navitem.active * {
      color: HighlightText;
    }
  }
</style>
