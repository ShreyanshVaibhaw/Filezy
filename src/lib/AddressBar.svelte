<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app, navigate, back, forward, up, refresh, setFilter, folderName, joinPath, THIS_PC, HOME } from "./store.svelte";

  let editing = $state(false);
  let editValue = $state("");
  let editInput: HTMLInputElement | undefined = $state();
  let searchValue = $state("");

  // Reset the search box text when the folder changes.
  $effect(() => {
    app.path;
    if (!app.filter) searchValue = "";
  });

  const segments = $derived.by(() => {
    if (!app.path) return [] as { name: string; path: string }[];
    if (app.path === THIS_PC) return [{ name: "This PC", path: THIS_PC }];
    if (app.path === HOME) return [{ name: "Home", path: HOME }];
    const parts = app.path.replace(/\\$/, "").split("\\");
    const out: { name: string; path: string }[] = [];
    let acc = "";
    for (const part of parts) {
      acc = acc ? acc + "\\" + part : part;
      out.push({ name: part.endsWith(":") ? diskName(part) : part, path: acc + (acc.endsWith(":") ? "\\" : "") });
    }
    return out;
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
        : (await invoke<string[]>("list_dirs", { path }).catch(() => [] as string[])).map(
            (n) => ({ name: n, path: joinPath(path, n) }),
          );
    if (!items.length) return;
    crumbMenu = { x: r.left, y: r.bottom + 6, items };
  }

  export function focus() {
    startEdit();
  }

  function startEdit() {
    editValue = app.path;
    editing = true;
    setTimeout(() => {
      editInput?.focus();
      editInput?.select();
    });
  }

  function commitEdit() {
    editing = false;
    const v = editValue.trim();
    if (v && v !== app.path) navigate(v);
  }
</script>

<div class="addressrow">
  <button class="navbtn" onclick={back} disabled={app.histIdx <= 0} title="Back (Alt + Left)">
    <span class="glyph">&#xE72B;</span>
  </button>
  <button class="navbtn" onclick={forward} disabled={app.histIdx >= app.histLen - 1} title="Forward (Alt + Right)">
    <span class="glyph">&#xE72A;</span>
  </button>
  <button class="navbtn" onclick={up} title="Up (Alt + Up)">
    <span class="glyph">&#xE74A;</span>
  </button>
  <button class="navbtn" onclick={() => refresh()} title="Refresh (F5)">
    <span class="glyph">&#xE72C;</span>
  </button>

  <div class="addressbox" role="textbox" tabindex="-1" onclick={(e) => { if (e.target === e.currentTarget) startEdit(); }}>
    {#if editing}
      <input
        bind:this={editInput}
        bind:value={editValue}
        onkeydown={(e) => {
          if (e.key === "Enter") commitEdit();
          else if (e.key === "Escape") editing = false;
        }}
        onblur={() => (editing = false)}
        spellcheck="false"
      />
    {:else}
      <span class="glyph pcicon">&#xE977;</span>
      {#if hiddenSegs.length}
        <button class="crumb chevbtn" onclick={toggleOverflow} title="Previous locations">
          <span class="glyph">&#xE712;</span>
        </button>
      {/if}
      <button class="crumbchevbtn" onclick={(e) => openCrumbMenu(THIS_PC, e)} title="This PC">
        <span class="glyph crumbchev">&#xE76C;</span>
      </button>
      {#each shownSegs as seg (seg.path)}
        <button class="crumb" onclick={() => navigate(seg.path)}>{seg.name}</button>
        <button class="crumbchevbtn" onclick={(e) => openCrumbMenu(seg.path, e)}>
          <span class="glyph crumbchev">&#xE76C;</span>
        </button>
      {/each}
      <div class="crumbfill" role="none" onclick={startEdit}></div>
    {/if}
  </div>

  <div class="searchbox">
    <span class="glyph searchicon">&#xE721;</span>
    <input
      placeholder={"Search " + (app.path ? folderName(app.path) : "")}
      bind:value={searchValue}
      oninput={() => setFilter(searchValue)}
      onkeydown={(e) => {
        if (e.key === "Escape") {
          searchValue = "";
          setFilter("");
        }
      }}
    />
  </div>
</div>

{#if crumbMenu}
  <div class="menu crumbdrop" style="left:{crumbMenu.x}px; top:{crumbMenu.y}px">
    {#each crumbMenu.items as it (it.path)}
      <button class="menu-item" onclick={() => { crumbMenu = null; navigate(it.path); }}>
        <span class="glyph">&#xE8B7;</span> {it.name}
      </button>
    {/each}
  </div>
{/if}

{#if overflowOpen}
  <div class="menu" style="left:{overflowX}px; top:{overflowY}px">
    {#each [...hiddenSegs].reverse() as seg (seg.path)}
      <button class="menu-item" onclick={() => { overflowOpen = false; navigate(seg.path); }}>
        <span class="glyph">&#xE8B7;</span> {seg.name}
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
  .searchbox {
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
</style>
