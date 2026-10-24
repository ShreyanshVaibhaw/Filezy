<script lang="ts">
  import { msg } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { SvelteSet } from "svelte/reactivity";
  import { app, recycleItems, loadRecycle, say, RECYCLE } from "./store.svelte";

  // Selection by the bin's own item ids: two deleted "report.txt" are two rows.
  const selIds = new SvelteSet<string>();
  let anchor = $state(-1);
  let busy = $state(false);

  // Drop selected ids that left the bin (restored elsewhere, emptied).
  $effect(() => {
    const present = new Set(recycleItems.list.map((it) => it.id));
    for (const id of selIds) if (!present.has(id)) selIds.delete(id);
  });

  function rowClick(i: number, ev: MouseEvent) {
    const id = recycleItems.list[i].id;
    if (ev.shiftKey && anchor >= 0) {
      if (!ev.ctrlKey) selIds.clear();
      const [a, b] = [Math.min(anchor, i), Math.max(anchor, i)];
      for (let k = a; k <= b; k++) selIds.add(recycleItems.list[k].id);
      return;
    }
    if (ev.ctrlKey) {
      if (selIds.has(id)) selIds.delete(id);
      else selIds.add(id);
    } else {
      selIds.clear();
      selIds.add(id);
    }
    anchor = i;
  }

  async function run(cmd: "recycle_restore" | "recycle_delete", ids: string[], done: string) {
    if (!ids.length || busy) return;
    busy = true;
    try {
      const n = await invoke<number>(cmd, { ids });
      // The shell finishes in the background; give it a moment before re-listing.
      await new Promise((r) => setTimeout(r, 300));
      await loadRecycle();
      const left = ids.filter((id) => recycleItems.list.some((it) => it.id === id)).length;
      if (!left) say(`${done} ${n === 1 ? "1 item" : `${n.toLocaleString()} items`}`);
      else if (left < ids.length) app.status = `${done} ${ids.length - left} of ${ids.length} items.`;
    } catch (e) {
      app.status = String(e);
      await loadRecycle();
    } finally {
      busy = false;
    }
  }

  const restore = () => run("recycle_restore", [...selIds], "Restored");
  const restoreAll = () => run("recycle_restore", recycleItems.list.map((it) => it.id), "Restored");
  // Windows asks "permanently delete?" itself.
  const deleteSel = () => run("recycle_delete", [...selIds], "Permanently deleted");

  async function emptyBin() {
    try {
      // Windows shows its own confirmation dialog before deleting anything.
      await invoke("recycle_empty");
    } catch {
      // cancelled or failed: the re-list shows what is left
    }
    await loadRecycle();
  }

  // The row the keyboard is on.
  let focus = $state(-1);

  function moveTo(i: number, shift: boolean) {
    const n = recycleItems.list.length;
    if (!n) return;
    i = Math.max(0, Math.min(n - 1, i));
    if (shift && anchor >= 0) {
      selIds.clear();
      const [a, b] = [Math.min(anchor, i), Math.max(anchor, i)];
      for (let k = a; k <= b; k++) selIds.add(recycleItems.list[k].id);
    } else {
      selIds.clear();
      selIds.add(recycleItems.list[i].id);
      anchor = i;
    }
    focus = i;
    document.getElementById(`bin-${i}`)?.scrollIntoView({ block: "nearest" });
  }

  function onKeydown(ev: KeyboardEvent) {
    const n = recycleItems.list.length;
    const cur = focus >= 0 ? focus : -1;
    const page = Math.max(1, Math.floor(((ev.currentTarget as HTMLElement).clientHeight || 340) / 34) - 1);
    let handled = true;
    if (ev.key === "Delete") deleteSel();
    else if (ev.key === "Enter") restore();
    else if (ev.key === "a" && ev.ctrlKey) for (const it of recycleItems.list) selIds.add(it.id);
    else if (ev.key === "ArrowDown") moveTo(cur + 1, ev.shiftKey);
    else if (ev.key === "ArrowUp") moveTo(cur < 0 ? 0 : cur - 1, ev.shiftKey);
    else if (ev.key === "PageDown") moveTo(cur + page, ev.shiftKey);
    else if (ev.key === "PageUp") moveTo(cur - page, ev.shiftKey);
    else if (ev.key === "Home") moveTo(0, ev.shiftKey);
    else if (ev.key === "End") moveTo(n - 1, ev.shiftKey);
    else handled = false;
    if (handled) {
      ev.preventDefault();
      ev.stopPropagation();
    }
  }
</script>

<div class="recyclepane" data-recycle data-drop={RECYCLE} data-drop-label="Recycle Bin" data-drop-quiet>
  <div class="actions">
    <button class="abtn" disabled={!selIds.size || busy} onclick={restore}>
      <span class="glyph" aria-hidden="true">&#xE777;</span>{msg("Restore selected")}
    </button>
    <button class="abtn" disabled={!recycleItems.list.length || busy} onclick={restoreAll}>
      <span class="glyph" aria-hidden="true">&#xE777;</span>{msg("Restore all items")}
    </button>
    <button class="abtn" disabled={!selIds.size || busy} onclick={deleteSel}>
      <span class="glyph" aria-hidden="true">&#xE74D;</span>{msg("Delete selected")}
    </button>
    <button class="abtn" disabled={!recycleItems.list.length || busy} onclick={emptyBin}>
      <span class="glyph" aria-hidden="true">&#xE74D;</span>{msg("Empty Recycle Bin")}
    </button>
  </div>

  {#if !recycleItems.list.length}
    <div class="empty">{recycleItems.loading ? msg("Loading…") : msg("Recycle Bin is empty.")}</div>
  {:else}
    {#if recycleItems.total > recycleItems.list.length}
      <div class="capped">
        {msg("Showing the first {shown} of {n} items.", { shown: recycleItems.list.length.toLocaleString(), n: recycleItems.total.toLocaleString() })}
      </div>
    {/if}
    <div class="rheader">
      <span class="c name">{msg("Name")}</span>
      <span class="c">{msg("Original location")}</span>
      <span class="c">{msg("Date deleted")}</span>
      <span class="c size">{msg("Size")}</span>
    </div>
    <div
      class="rows"
      role="listbox"
      aria-multiselectable="true"
      aria-label={msg("Recycle Bin")}
      tabindex="0"
      aria-activedescendant={focus >= 0 ? `bin-${focus}` : undefined}
      onkeydown={onKeydown}
    >
      {#each recycleItems.list as it, i (it.id)}
        <div
          class="rrow"
          class:selected={selIds.has(it.id)}
          class:focused={focus === i}
          id={`bin-${i}`}
          role="option"
          aria-selected={selIds.has(it.id)}
          aria-setsize={recycleItems.list.length}
          aria-posinset={i + 1}
          tabindex="-1"
          onmousedown={(ev) => { ev.preventDefault(); (ev.currentTarget.parentElement as HTMLElement).focus(); }}
          onclick={(ev) => { rowClick(i, ev); focus = i; }}
          onkeydown={(ev) => { if (ev.key === "Enter") restore(); }}
          ondblclick={restore}
          title={it.original}
        >
          <span class="c name">{it.name}</span>
          <span class="c dim">{it.original}</span>
          <span class="c dim">{it.deleted}</span>
          <span class="c size dim">{it.size}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .recyclepane {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 8px 12px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding: 4px 4px 10px;
    flex-shrink: 0;
  }
  .abtn {
    height: 32px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    font-size: 13px;
    border: 1px solid var(--stroke-strong);
    border-radius: 4px;
  }
  .abtn:hover:not(:disabled) {
    background: var(--hover);
  }
  .abtn .glyph {
    font-size: 14px;
    color: var(--text-2);
  }
  .abtn:disabled .glyph {
    color: var(--text-disabled);
  }
  .empty {
    flex: 1;
    display: grid;
    place-items: center;
    font-size: 13px;
    color: var(--text-3);
  }
  .capped {
    font-size: 12px;
    color: var(--text-3);
    padding: 0 10px 6px;
  }
  .rheader,
  .rrow {
    display: grid;
    grid-template-columns: minmax(160px, 1fr) minmax(200px, 1.4fr) 150px 90px;
    gap: 8px;
    align-items: center;
    padding: 0 10px;
  }
  .rheader {
    height: 34px;
    font-size: 13px;
    color: var(--text-2);
    border-bottom: 1px solid var(--stroke);
    flex-shrink: 0;
  }
  .rows {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    outline: none;
  }
  .rrow {
    width: 100%;
    height: 34px;
    font-size: 13px;
    text-align: left;
    border-radius: 4px;
  }
  .rrow:hover {
    background: var(--hover);
  }
  .rrow.selected {
    background: var(--sel);
  }
  .rows:focus-visible .rrow.focused {
    outline: 1px solid var(--text-2);
    outline-offset: -1px;
  }
  .c {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .c.size {
    text-align: right;
  }
  .dim {
    color: var(--text-3);
    font-size: 12px;
  }
  @media (forced-colors: active) {
    .rrow.selected {
      forced-color-adjust: none;
      background: Highlight;
      color: HighlightText;
    }
    .rrow.selected * {
      color: HighlightText;
    }
  }
</style>
