<script lang="ts">
  import { app, navigate, folderName, pinRemove, THIS_PC, HOME } from "./store.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";

  let pcOpen = $state(true);

  // Lazy folder tree: path -> children names (null = loading).
  const kids = new SvelteMap<string, string[] | null>();
  const open = new SvelteSet<string>();

  function joinDir(parent: string, name: string): string {
    return parent.replace(/\\$/, "") + "\\" + name;
  }

  function toggle(path: string, ev: MouseEvent) {
    ev.stopPropagation();
    if (open.has(path)) {
      open.delete(path);
      return;
    }
    open.add(path);
    if (!kids.has(path)) {
      kids.set(path, null);
      invoke<string[]>("list_dirs", { path }).then((d) => kids.set(path, d));
    }
  }

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

  let pinCtx = $state<{ x: number; y: number; path: string } | null>(null);
  function onPinContext(ev: MouseEvent, path: string) {
    ev.preventDefault();
    pinCtx = { x: ev.clientX, y: ev.clientY, path };
  }
</script>

{#snippet treeNode(path: string, label: string, glyph: string, depth: number)}
  <button
    class="navitem"
    class:active={isActive(path)}
    style="padding-left: {6 + depth * 16}px"
    onclick={() => navigate(path)}
    title={path}
  >
    <span
      class="glyph chevslot chev"
      role="none"
      onmousedown={(ev) => toggle(path, ev)}
      onclick={(ev) => ev.stopPropagation()}
    >
      {open.has(path) ? "" : ""}
    </span>
    <span class="glyph icon">{glyph}</span>
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

<div class="navpane">
  <button class="navitem" class:active={app.path === HOME} onclick={() => navigate(HOME)}>
    <span class="chevslot"></span>
    <span class="glyph icon">&#xE80F;</span>
    <span class="label">Home</span>
  </button>

  <div class="sep"></div>

  {#each app.specials as [name, path] (path)}
    <button class="navitem" class:active={isActive(path)} onclick={() => navigate(path)}>
      <span class="chevslot"></span>
      <span class="glyph icon">{specialGlyphs[name] ?? ""}</span>
      <span class="label">{name}</span>
      <span class="glyph pin">&#xE840;</span>
    </button>
  {/each}

  {#each app.pins as p (p)}
    <button
      class="navitem"
      class:active={isActive(p)}
      onclick={() => navigate(p)}
      oncontextmenu={(ev) => onPinContext(ev, p)}
      title={p}
    >
      <span class="chevslot"></span>
      <span class="glyph icon">&#xE8B7;</span>
      <span class="label">{folderName(p)}</span>
      <span class="glyph pin">&#xE840;</span>
    </button>
  {/each}

  <div class="sep"></div>

  <button class="navitem" class:active={app.path === THIS_PC} onclick={() => navigate(THIS_PC)}>
    <span
      class="glyph chevslot chev"
      role="none"
      onmousedown={(ev) => { ev.stopPropagation(); pcOpen = !pcOpen; }}
      onclick={(ev) => ev.stopPropagation()}
    >
      {pcOpen ? "" : ""}
    </span>
    <span class="glyph icon">&#xE977;</span>
    <span class="label">This PC</span>
  </button>
  {#if pcOpen}
    {#each app.drives as d (d.path)}
      {@render treeNode(d.path, d.name, "", 1)}
    {/each}
  {/if}
</div>

{#if pinCtx}
  <div class="menu" style="left:{pinCtx.x}px; top:{pinCtx.y}px">
    <button
      class="menu-item"
      onclick={() => { pinRemove(pinCtx!.path); pinCtx = null; }}
    >
      <span class="glyph">&#xE77A;</span> Unpin from Quick access
    </button>
  </div>
{/if}

<svelte:window onmousedown={(e) => { if (pinCtx && !(e.target as HTMLElement).closest(".menu")) pinCtx = null; }} />

<style>
  .navpane {
    width: 240px;
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
  .navitem.active {
    background: var(--sel);
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
</style>
