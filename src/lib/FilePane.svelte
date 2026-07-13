<script lang="ts">
  import {
    app, rows, sel, ensureRange, requestMeta, iconOf, typeOf, fmtDate, fmtSizeKB,
    openEntry, sortBy, hooks, commitRename, copySel, paste, del, newFolder, refresh,
    startRename, selectAll, isSelected, clearSel, selIndices, refreshClipState,
    requestThumb, thumbOf, entryPath, collapsedGroups, loadNewItems, newFile, newItems,
    extMeta, pinAdd, pinRemove, displayName, newTab,
  } from "./store.svelte";
  import type { Entry } from "./store.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { onMount } from "svelte";

  interface ShellItem {
    id: number;
    text: string;
    verb: string | null;
    sep: boolean;
    disabled: boolean;
    sub: ShellItem[] | null;
  }

  // View geometry. vertical = icon above label (icon grids); tile = icon left, two text lines.
  interface ViewDef {
    grid: boolean;
    rowH: number;
    cellW: number;
    icon: number;
    vertical?: boolean;
    tile?: boolean;
    flow?: "column"; // Explorer's List view: top-to-bottom, then next column
  }
  const VIEWS: Record<string, ViewDef> = {
    details: { grid: false, rowH: 40, cellW: 0, icon: 16 },
    list: { grid: true, flow: "column", rowH: 28, cellW: 250, icon: 16 },
    icons_s: { grid: true, rowH: 32, cellW: 250, icon: 16 },
    icons_m: { grid: true, rowH: 118, cellW: 102, icon: 48, vertical: true },
    icons_l: { grid: true, rowH: 166, cellW: 140, icon: 96, vertical: true },
    icons_xl: { grid: true, rowH: 300, cellW: 268, icon: 256, vertical: true },
    tiles: { grid: true, rowH: 68, cellW: 272, icon: 48, tile: true },
  };

  const HEADER = 40;

  let scroller: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let scrollLeft = $state(0);
  let viewH = $state(600);
  let gridW = $state(800);

  const v = $derived(VIEWS[app.view] ?? VIEWS.details);
  // items per column for column flow; visual columns otherwise
  const ipc = $derived(v.flow === "column" ? Math.max(1, Math.floor(viewH / v.rowH)) : 1);
  const cols = $derived(
    !v.grid
      ? 1
      : v.flow === "column"
        ? Math.max(1, Math.ceil(app.total / ipc))
        : Math.max(1, Math.floor((gridW - 16) / v.cellW)),
  );

  // Grouped details view: display rows = group headers + entries of expanded groups.
  const grouped = $derived(!v.grid && app.groupKey !== "none" && app.groups.length > 0);
  const gview = $derived.by(() => {
    if (!grouped) return null;
    let acc = 0;
    const segs = app.groups.map((g) => {
      const d = acc;
      acc += 1 + (collapsedGroups.has(g.label) ? 0 : g.count);
      return { g, d };
    });
    return { segs, total: acc };
  });
  const dispTotal = $derived(gview ? gview.total : app.total);
  const totalRows = $derived(Math.ceil(dispTotal / cols));

  /** Map a display row to a group header or an entry index. */
  function displayItem(d: number): { header?: Group; idx: number } {
    const gv = gview;
    if (!gv) return { idx: d };
    let lo = 0,
      hi = gv.segs.length - 1,
      k = 0;
    while (lo <= hi) {
      const m = (lo + hi) >> 1;
      if (gv.segs[m].d <= d) {
        k = m;
        lo = m + 1;
      } else hi = m - 1;
    }
    const s = gv.segs[k];
    if (d === s.d) return { header: s.g, idx: -1 };
    return { idx: s.g.start + (d - s.d - 1) };
  }
  interface Group {
    label: string;
    start: number;
    count: number;
  }
  function toggleGroup(label: string) {
    if (collapsedGroups.has(label)) collapsedGroups.delete(label);
    else collapsedGroups.add(label);
  }

  const range = $derived.by(() => {
    if (v.flow === "column") {
      const cf = Math.max(0, Math.floor(scrollLeft / v.cellW) - 1);
      const cl = Math.min(cols, Math.ceil((scrollLeft + gridW) / v.cellW) + 1);
      return { first: Math.min(app.total, cf * ipc), last: Math.min(app.total, cl * ipc) };
    }
    const fr = Math.max(0, Math.floor(scrollTop / v.rowH) - 6);
    const lr = Math.min(totalRows, Math.ceil((scrollTop + viewH) / v.rowH) + 6);
    return { first: fr * cols, last: Math.min(dispTotal, lr * cols) };
  });
  const visible = $derived.by(() => {
    const out: number[] = [];
    for (let i = range.first; i < range.last; i++) out.push(i);
    return out;
  });

  // Entry-index span behind the visible display rows (identical when ungrouped).
  const entrySpan = $derived.by(() => {
    if (!gview) return range;
    let min = Number.MAX_SAFE_INTEGER,
      max = -1;
    for (let d = range.first; d < range.last; d++) {
      const it = displayItem(d);
      if (it.header) continue;
      if (it.idx < min) min = it.idx;
      if (it.idx > max) max = it.idx;
    }
    return max < 0 ? { first: 0, last: 0 } : { first: min, last: max + 1 };
  });

  $effect(() => {
    ensureRange(entrySpan.first, entrySpan.last);
  });
  $effect(() => {
    for (let i = entrySpan.first; i < entrySpan.last; i++) {
      const e = rows[i];
      if (!e) continue;
      requestMeta(e);
      if (v.icon >= 32) requestThumb(i, e, v.icon);
    }
  });
  $effect(() => {
    hooks.gridCols = v.flow === "column" ? 1 : cols;
    hooks.hStep = v.flow === "column" ? ipc : 1;
  });

  hooks.scrollTo = (px) => {
    if (scroller) {
      scroller.scrollTop = px;
      scroller.scrollLeft = 0;
    }
  };
  hooks.getScroll = () => scroller?.scrollTop ?? 0;
  hooks.scrollToIndex = (i) => {
    if (!scroller) return;
    if (v.flow === "column") {
      const left = Math.floor(i / ipc) * v.cellW;
      if (left < scroller.scrollLeft) scroller.scrollLeft = left;
      else if (left + v.cellW > scroller.scrollLeft + gridW)
        scroller.scrollLeft = left + v.cellW - gridW;
      return;
    }
    let d = i;
    if (gview) {
      const seg = gview.segs.find(({ g }) => i >= g.start && i < g.start + g.count);
      if (!seg) return;
      // If the target's group is collapsed, reveal its header - never silently
      // re-expand a group the user deliberately collapsed.
      d = collapsedGroups.has(seg.g.label) ? seg.d : seg.d + 1 + (i - seg.g.start);
    }
    const top = Math.floor(d / cols) * v.rowH;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (top + v.rowH > scroller.scrollTop + viewH) scroller.scrollTop = top + v.rowH - viewH;
  };

  function rowMouseDown(i: number, ev: MouseEvent) {
    // Middle-click a folder: open it in a new tab (Explorer/browser behavior).
    if (ev.button === 1) {
      const e = rows[i];
      if (e?.is_dir) newTab(entryPath(e));
      return;
    }
    if (ev.button === 2) {
      if (!isSelected(i)) {
        clearSel();
        sel.add(i);
        app.anchor = i;
      }
      return;
    }
    if (ev.ctrlKey && !app.allSelected) {
      if (sel.has(i)) sel.delete(i);
      else sel.add(i);
      app.anchor = i;
    } else if (ev.shiftKey) {
      clearSel();
      const [a, b] = [Math.min(app.anchor, i), Math.max(app.anchor, i)];
      for (let k = a; k <= b; k++) sel.add(k);
    } else {
      clearSel();
      sel.add(i);
      app.anchor = i;
    }
  }

  let ctx = $state<{ x: number; y: number; onRow: boolean } | null>(null);
  let ctxItems = $state<ShellItem[] | null>(null);
  let shellFailed = $state(false);
  let openSub = $state(-1);

  async function onContext(ev: MouseEvent, i: number | null) {
    ev.preventDefault();
    if (i !== null && !isSelected(i)) {
      clearSel();
      sel.add(i);
      app.anchor = i;
    }
    const x = Math.min(ev.clientX, window.innerWidth - 300);
    const y = Math.min(ev.clientY, window.innerHeight - 420);
    ctx = { x, y, onRow: i !== null };
    ctxItems = null;
    shellFailed = false;
    openSub = -1;
    if (i === null) {
      // Blank area: our own Win11-style menu (shell background verbs need Explorer's view host).
      shellFailed = true;
      ctxItems = [];
      blankNewOpen = false;
      loadNewItems();
      return;
    }
    // Build the shell menu against the folder the items actually live in. Search
    // results carry their own parent, so app.path would be the wrong directory;
    // key off the right-clicked item's folder and only include items from it.
    const anchor = rows[i];
    const dir = anchor?.parent ?? app.path;
    // ponytail: shell menu built for at most 100 items; huge selections get the first 100
    const names = selIndices()
      .map((k) => rows[k])
      .filter((e) => e && (e.parent ?? app.path) === dir)
      .slice(0, 100)
      .map((e) => e!.name);
    try {
      ctxItems = await invoke<ShellItem[]>("shell_menu", { dir, names });
    } catch {
      shellFailed = true;
      ctxItems = [];
    }
  }

  // Standard verbs run through our native implementations; only third-party
  // extension verbs go through IContextMenu::InvokeCommand (shell-built-in verbs
  // fail without Explorer's view/site chain).
  async function shellInvoke(it: ShellItem) {
    ctx = null;
    const v2 = (it.verb ?? "").toLowerCase();
    const first = selIndices()[0];
    try {
      if (v2 === "open" && first !== undefined) return openEntry(first);
      if (v2 === "cut") return void (await copySel(true));
      if (v2 === "copy") return void (await copySel(false));
      if (v2 === "copyaspath") {
        await invoke("copy_as_path", { indices: selIndices() });
        return refreshClipState();
      }
      if (v2 === "delete") return void (await del());
      if (v2 === "rename") return startRename();
      // Pin verbs act on OUR Quick Access, not real Explorer's.
      if (v2 === "pintohome" && first !== undefined) {
        const e = rows[first];
        if (e) pinAdd(entryPath(e));
        return;
      }
      if (v2 === "unpinfromhome" && first !== undefined) {
        const e = rows[first];
        if (e) pinRemove(entryPath(e));
        return;
      }
      if (v2 === "properties" && first !== undefined)
        return void (await invoke("show_properties", { index: first }));
      if (v2 === "openas" && first !== undefined)
        return void (await invoke("open_with", { index: first }));
      await invoke("shell_menu_invoke", { id: it.id });
    } catch (e) {
      app.status = String(e);
    } finally {
      setTimeout(() => {
        refresh();
        refreshClipState();
      }, 400);
    }
  }

  // Inline rename
  let renameValue = $state("");
  function renameInput(node: HTMLInputElement, name: string) {
    renameValue = name;
    node.focus();
    const dot = name.lastIndexOf(".");
    node.setSelectionRange(0, dot > 0 ? dot : name.length);
  }

  // Details columns: Name is fixed leftmost and flexes; the metadata columns
  // resize and reorder (Explorer keeps Name primary).
  type MetaKey = "modified" | "type" | "size";
  const META_LABEL: Record<MetaKey, string> = {
    modified: "Date modified",
    type: "Type",
    size: "Size",
  };
  const colW = $state<Record<MetaKey, number>>({ modified: 170, type: 150, size: 110 });
  let colOrder = $state<MetaKey[]>(["modified", "type", "size"]);
  const gridCols = $derived(`minmax(200px, 1fr) ${colOrder.map((k) => colW[k] + "px").join(" ")}`);

  function cellText(e: Entry, key: MetaKey): string {
    return key === "modified" ? fmtDate(e.modified) : key === "type" ? typeOf(e) : fmtSizeKB(e);
  }

  let headerEl: HTMLDivElement | undefined = $state();

  // Column resize (drag the divider at a header's right edge).
  let drag: { key: MetaKey; startX: number; startW: number } | null = null;
  function startResize(key: MetaKey, ev: MouseEvent) {
    ev.stopPropagation();
    ev.preventDefault();
    drag = { key, startX: ev.clientX, startW: colW[key] };
  }
  function onResizeMove(ev: MouseEvent) {
    if (!drag) return;
    colW[drag.key] = Math.max(60, drag.startW + ev.clientX - drag.startX);
  }
  function endResize() {
    drag = null;
  }

  // Column reorder (drag a metadata header onto another; live-swaps as you move).
  let colDrag: { key: MetaKey; startX: number; moved: boolean } | null = null;
  let suppressSort = false;
  function startColDrag(key: MetaKey, ev: MouseEvent) {
    if (ev.button !== 0) return;
    colDrag = { key, startX: ev.clientX, moved: false };
  }
  function colDragMove(ev: MouseEvent) {
    if (!colDrag || !headerEl) return;
    if (!colDrag.moved && Math.abs(ev.clientX - colDrag.startX) < 5) return;
    colDrag.moved = true;
    const cells = [...headerEl.querySelectorAll<HTMLElement>(".hcell[data-meta]")];
    const from = colOrder.indexOf(colDrag.key);
    let to = colOrder.length - 1;
    for (let idx = 0; idx < cells.length; idx++) {
      const r = cells[idx].getBoundingClientRect();
      if (ev.clientX < r.left + r.width / 2) {
        to = idx;
        break;
      }
    }
    if (to !== from) {
      const next = colOrder.filter((k) => k !== colDrag!.key);
      next.splice(to, 0, colDrag!.key);
      colOrder = next;
    }
  }
  function endColDrag() {
    if (colDrag?.moved) suppressSort = true;
    colDrag = null;
  }
  function headerClick(key: MetaKey | "name") {
    if (suppressSort) {
      suppressSort = false;
      return;
    }
    sortBy(key);
  }

  function blankTarget(e: MouseEvent): boolean {
    const t = e.target as HTMLElement;
    return t === e.currentTarget || t.classList.contains("canvas");
  }

  // ---- marquee (rubber-band) selection ----
  let marquee = $state<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  function marqueeStart(ev: MouseEvent) {
    if (ev.button !== 0 || !scroller) return;
    clearSel();
    const r = scroller.getBoundingClientRect();
    const x = ev.clientX - r.left + scroller.scrollLeft;
    const y = ev.clientY - r.top + scroller.scrollTop;
    marquee = { x0: x, y0: y, x1: x, y1: y };
  }
  function marqueeMove(ev: MouseEvent) {
    if (!marquee || !scroller) return;
    const r = scroller.getBoundingClientRect();
    marquee.x1 = Math.max(0, ev.clientX - r.left + scroller.scrollLeft);
    marquee.y1 = Math.max(0, ev.clientY - r.top + scroller.scrollTop);
    applyMarquee();
  }
  function marqueeEnd() {
    marquee = null;
  }
  const marqueeRect = $derived(
    marquee
      ? {
          x: Math.min(marquee.x0, marquee.x1),
          y: Math.min(marquee.y0, marquee.y1),
          w: Math.abs(marquee.x1 - marquee.x0),
          h: Math.abs(marquee.y1 - marquee.y0),
        }
      : null,
  );
  function applyMarquee() {
    const m = marqueeRect;
    if (!m) return;
    sel.clear();
    app.allSelected = false;
    if (!v.grid) {
      // details: rows span the full width, select by vertical intersection
      const dFirst = Math.max(0, Math.floor(m.y / v.rowH));
      const dLast = Math.min(dispTotal - 1, Math.floor((m.y + m.h) / v.rowH));
      for (let d = dFirst; d <= dLast; d++) {
        const it = displayItem(d);
        if (!it.header && it.idx >= 0 && it.idx < app.total) sel.add(it.idx);
      }
    } else {
      const cellW = v.cellW;
      const rFirst = Math.max(0, Math.floor(m.y / v.rowH));
      const rLast = Math.floor((m.y + m.h) / v.rowH);
      const cFirst = Math.max(0, Math.floor((m.x - 8) / cellW));
      const cLast = Math.floor((m.x + m.w - 8) / cellW);
      for (let r = rFirst; r <= rLast; r++) {
        for (let c = cFirst; c <= cLast; c++) {
          const i = v.flow === "column" ? c * ipc + r : r * cols + c;
          if (v.flow === "column" && r >= ipc) continue;
          if (v.flow !== "column" && c >= cols) continue;
          if (i >= 0 && i < app.total) sel.add(i);
        }
      }
    }
  }

  // Blank-menu "New >" submenu state
  let blankNewOpen = $state(false);

  // ---- OLE drag out ----
  let dragArm: { x: number; y: number } | null = null;
  function armDrag(ev: MouseEvent) {
    if (ev.button === 0 && !ev.ctrlKey && !ev.shiftKey) dragArm = { x: ev.clientX, y: ev.clientY };
  }
  function dragMove(ev: MouseEvent) {
    if (!dragArm) return;
    if (Math.abs(ev.clientX - dragArm.x) + Math.abs(ev.clientY - dragArm.y) > 6) {
      dragArm = null;
      invoke("start_drag", { indices: selIndices() }).catch((e) => (app.status = String(e)));
    }
  }

  // ---- native drop in ----
  let dropIdx = $state(-1);
  function hitTest(pos: { x: number; y: number }): number {
    const s = window.devicePixelRatio || 1;
    const el = document
      .elementFromPoint(pos.x / s, pos.y / s)
      ?.closest("[data-idx]") as HTMLElement | null;
    const i = el ? +el.dataset.idx! : -1;
    return i >= 0 && rows[i]?.is_dir ? i : -1;
  }
  async function handleDrop(paths: string[], pos: { x: number; y: number }) {
    const idx = hitTest(pos);
    dropIdx = -1;
    const e = idx >= 0 ? rows[idx] : undefined;
    const dest = e ? entryPath(e) : app.path;
    try {
      await invoke("drop_paths", { paths, dest });
    } catch (err) {
      app.status = String(err);
    }
    refresh();
  }
  onMount(() => {
    let un: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((ev) => {
        const p = ev.payload as any;
        if (p.type === "over") dropIdx = hitTest(p.position);
        else if (p.type === "drop") handleDrop(p.paths, p.position);
        else dropIdx = -1;
      })
      .then((u) => (un = u));
    return () => un?.();
  });
</script>

<svelte:window
  onmousedown={(e) => { if (ctx && !(e.target as HTMLElement).closest(".menu")) ctx = null; }}
  onmousemove={(e) => { onResizeMove(e); dragMove(e); colDragMove(e); marqueeMove(e); }}
  onmouseup={() => { endResize(); dragArm = null; endColDrag(); marqueeEnd(); }}
/>

<div class="pane">
  {#if !v.grid}
    <div class="header" bind:this={headerEl} style="height:{HEADER}px; grid-template-columns:{gridCols}">
      <button class="hcell" onclick={() => headerClick("name")}>
        {#if app.sortKey === "name"}
          <span class="glyph caret">{app.asc ? "" : ""}</span>
        {/if}
        <span class="hlabel">Name</span>
      </button>
      {#each colOrder as key (key)}
        <button
          class="hcell"
          class:coldragging={colDrag?.key === key && colDrag.moved}
          data-meta={key}
          onmousedown={(ev) => startColDrag(key, ev)}
          onclick={() => headerClick(key)}
        >
          {#if app.sortKey === key}
            <span class="glyph caret">{app.asc ? "" : ""}</span>
          {/if}
          <span class="hlabel">{META_LABEL[key]}</span>
          <div class="hresize" role="none" onmousedown={(ev) => startResize(key, ev)}></div>
        </button>
      {/each}
    </div>
  {/if}

  <div
    class="scroller"
    bind:this={scroller}
    bind:clientHeight={viewH}
    bind:clientWidth={gridW}
    class:colflow={v.flow === "column"}
    onscroll={() => { scrollTop = scroller!.scrollTop; scrollLeft = scroller!.scrollLeft; }}
    onmousedown={(e) => { if (blankTarget(e)) marqueeStart(e); }}
    oncontextmenu={(e) => { if (blankTarget(e)) onContext(e, null); }}
    role="listbox"
    tabindex="-1"
  >
    {#if app.total === 0}
      <div class="emptyfolder">{app.searching ? "Searching…" : "This folder is empty."}</div>
    {/if}
    <div
      class="canvas"
      style={v.flow === "column"
        ? `height:${ipc * v.rowH}px; width:${cols * v.cellW + 16}px`
        : `height:${totalRows * v.rowH}px`}
    >
      {#each visible as d (app.version + ":" + app.view + ":" + d)}
        {@const item = displayItem(d)}
        {@const i = item.idx}
        {@const e = i >= 0 ? rows[i] : undefined}
        {#if !v.grid && item.header}
          {@const hdr = item.header}
          <div
            class="grouphdr"
            style="transform: translateY({d * v.rowH}px); height:{v.rowH}px"
            role="button"
            tabindex="-1"
            onmousedown={() => toggleGroup(hdr.label)}
          >
            <span class="glyph gchev">{collapsedGroups.has(hdr.label) ? "" : ""}</span>
            <span class="ghlabel">{hdr.label}</span>
            <span class="ghcount">({hdr.count})</span>
          </div>
        {:else if !v.grid}
          <div
            class="vrow"
            class:selected={isSelected(i)}
            class:drophover={dropIdx === i}
            title={e?.parent ?? ""}
            data-idx={i}
            style="transform: translateY({d * v.rowH}px); height:{v.rowH}px; grid-template-columns:{gridCols}"
            onmousedown={(ev) => { if (e) { rowMouseDown(i, ev); armDrag(ev); } }}
            ondblclick={() => e && app.renaming !== i && openEntry(i)}
            oncontextmenu={(ev) => e && onContext(ev, i)}
            role="option"
            aria-selected={isSelected(i)}
            tabindex="-1"
          >
            {#if e}
              <div class="cell name" class:dim={e.hidden}>
                {#if iconOf(e)}
                  <img class="fileicon" src={iconOf(e)} alt="" width="16" height="16" />
                {:else}
                  <span class="glyph fileicon placeholder">{e.is_dir ? "" : ""}</span>
                {/if}
                {#if app.renaming === i}
                  <input
                    class="rename"
                    use:renameInput={e.name}
                    bind:value={renameValue}
                    onkeydown={(ev) => {
                      ev.stopPropagation();
                      if (ev.key === "Enter") commitRename(i, renameValue);
                      else if (ev.key === "Escape") app.renaming = -1;
                    }}
                    onblur={() => commitRename(i, renameValue)}
                    onmousedown={(ev) => ev.stopPropagation()}
                    ondblclick={(ev) => ev.stopPropagation()}
                    spellcheck="false"
                  />
                {:else}
                  <span class="fname">{displayName(e)}</span>
                {/if}
              </div>
              {#each colOrder as key (key)}
                <div class="cell sub" class:size={key === "size"}>{cellText(e, key)}</div>
              {/each}
            {/if}
          </div>
        {:else}
          {@const r = v.flow === "column" ? i % ipc : Math.floor(i / cols)}
          {@const c = v.flow === "column" ? Math.floor(i / ipc) : i % cols}
          <div
            class="gcell"
            class:vert={v.vertical}
            class:tile={v.tile}
            class:selected={isSelected(i)}
            class:drophover={dropIdx === i}
            data-idx={i}
            style="transform: translate({8 + c * v.cellW}px, {r * v.rowH}px); width:{v.cellW - 6}px; height:{v.rowH - 6}px"
            onmousedown={(ev) => { if (e) { rowMouseDown(i, ev); armDrag(ev); } }}
            ondblclick={() => e && app.renaming !== i && openEntry(i)}
            oncontextmenu={(ev) => e && onContext(ev, i)}
            role="option"
            aria-selected={isSelected(i)}
            tabindex="-1"
          >
            {#if e}
              {@const src = v.icon >= 32 ? thumbOf(e, v.icon) : iconOf(e)}
              <div class="gicon" style={v.vertical ? `height:${v.icon + 8}px` : `width:${v.icon}px`}>
                {#if src}
                  <img src={src} alt="" style="max-width:{v.icon}px; max-height:{v.icon}px" />
                {:else}
                  <span class="glyph placeholder" style="font-size:{Math.max(16, v.icon * 0.7)}px">
                    {e.is_dir ? "" : ""}
                  </span>
                {/if}
              </div>
              <div class="glabel" class:dim={e.hidden}>
                {#if app.renaming === i}
                  <input
                    class="rename"
                    use:renameInput={e.name}
                    bind:value={renameValue}
                    onkeydown={(ev) => {
                      ev.stopPropagation();
                      if (ev.key === "Enter") commitRename(i, renameValue);
                      else if (ev.key === "Escape") app.renaming = -1;
                    }}
                    onblur={() => commitRename(i, renameValue)}
                    onmousedown={(ev) => ev.stopPropagation()}
                    ondblclick={(ev) => ev.stopPropagation()}
                    spellcheck="false"
                  />
                {:else}
                  <span class="gname" class:clamp2={v.vertical}>{displayName(e)}</span>
                  {#if v.tile}
                    <span class="gsub">{typeOf(e)}</span>
                    {#if !e.is_dir}<span class="gsub">{fmtSizeKB(e)}</span>{/if}
                  {/if}
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      {/each}
    </div>
    {#if marqueeRect && (marqueeRect.w > 2 || marqueeRect.h > 2)}
      <div
        class="marquee"
        style="left:{marqueeRect.x}px; top:{marqueeRect.y}px; width:{marqueeRect.w}px; height:{marqueeRect.h}px"
      ></div>
    {/if}
    {#if app.total === 0}
      <div class="empty">{app.filter ? "No items match your search." : "This folder is empty."}</div>
    {/if}
  </div>
</div>

{#if ctx}
  <div class="menu ctxmenu" style="left:{ctx.x}px; top:{ctx.y}px">
    {#if ctx.onRow}
      <div class="strip">
        <button title="Cut" onclick={() => { ctx = null; copySel(true); }}><span class="glyph">&#xE8C6;</span></button>
        <button title="Copy" onclick={() => { ctx = null; copySel(false); }}><span class="glyph">&#xE8C8;</span></button>
        <button title="Rename" onclick={() => { ctx = null; startRename(); }}><span class="glyph">&#xE8AC;</span></button>
        <button title="Delete" onclick={() => { ctx = null; del(); }}><span class="glyph">&#xE74D;</span></button>
      </div>
      <div class="menu-sep"></div>
    {/if}
    {#if ctxItems === null}
      <div class="menuload">Loading…</div>
    {:else if shellFailed || ctxItems.length === 0}
      {#if ctx.onRow}
        <button class="menu-item" onclick={() => { const i = app.allSelected ? 0 : [...sel][0]; ctx = null; if (i !== undefined) openEntry(i); }}>
          <span class="glyph">&#xE8E5;</span> Open
        </button>
      {:else}
        <button class="menu-item" onclick={() => { ctx = null; refresh(); }}>
          <span class="glyph">&#xE72C;</span> Refresh
        </button>
        <button class="menu-item" disabled={!app.canPaste} onclick={() => { ctx = null; paste(); }}>
          <span class="glyph">&#xE77F;</span> Paste
        </button>
        <div class="menu-sep"></div>
        <div class="subwrap" role="none" onmouseenter={() => (blankNewOpen = true)} onmouseleave={() => (blankNewOpen = false)}>
          <button class="menu-item wide">
            <span class="glyph">&#xE710;</span>
            <span class="mtext">New</span>
            <span class="glyph subchev">&#xE76C;</span>
          </button>
          {#if blankNewOpen}
            <div class="menu submenu">
              <button class="menu-item wide" onclick={() => { ctx = null; newFolder(); }}>
                {#if extMeta.get("//dir")?.icon}
                  <img src={extMeta.get("//dir")?.icon} alt="" width="16" height="16" />
                {:else}
                  <span class="glyph">&#xE8B7;</span>
                {/if}
                <span class="mtext">Folder</span>
              </button>
              {#if newItems.list?.length}
                <div class="menu-sep"></div>
                {#each newItems.list as it (it.ext)}
                  <button class="menu-item wide" onclick={() => { ctx = null; newFile(it.ext); }}>
                    {#if extMeta.get(it.ext)?.icon}
                      <img src={extMeta.get(it.ext)?.icon} alt="" width="16" height="16" />
                    {:else}
                      <span class="glyph">&#xE7C3;</span>
                    {/if}
                    <span class="mtext">{it.name}</span>
                  </button>
                {/each}
              {/if}
            </div>
          {/if}
        </div>
        <button class="menu-item" onclick={() => { ctx = null; selectAll(); }}>
          <span class="glyph">&#xE8B3;</span> Select all
        </button>
      {/if}
    {:else}
      <div class="scrollwrap">
        {#each ctxItems as it, idx (idx)}
          {#if it.sep}
            <div class="menu-sep"></div>
          {:else}
            <div class="subwrap" role="none" onmouseenter={() => (openSub = it.sub ? idx : -1)}>
              <button class="menu-item wide" disabled={it.disabled} onclick={() => !it.sub && shellInvoke(it)}>
                <span class="mtext">{it.text}</span>
                {#if it.sub}<span class="glyph subchev">&#xE76C;</span>{/if}
              </button>
              {#if it.sub && openSub === idx}
                <div class="menu submenu">
                  {#each it.sub as s, sidx (sidx)}
                    {#if s.sep}
                      <div class="menu-sep"></div>
                    {:else}
                      <button class="menu-item wide" disabled={s.disabled} onclick={() => shellInvoke(s)}>
                        <span class="mtext">{s.text}</span>
                      </button>
                    {/if}
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .pane {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .header,
  .vrow {
    display: grid;
    grid-template-columns: minmax(200px, 1fr) 170px 150px 110px;
  }
  .header {
    flex-shrink: 0;
    padding: 0 12px 0 16px;
    border-bottom: 1px solid var(--stroke);
  }
  .hcell {
    position: relative;
    display: flex;
    align-items: center;
    padding: 0 8px;
    font-size: 13px;
    color: var(--text-2);
    border-radius: 0;
    border-right: 1px solid transparent;
    justify-content: flex-start;
  }
  .hcell:hover {
    background: var(--hover);
    border-right-color: var(--stroke);
  }
  .hresize {
    position: absolute;
    right: -3px;
    top: 4px;
    bottom: 4px;
    width: 7px;
    cursor: col-resize;
    z-index: 2;
  }
  .hcell.coldragging {
    background: var(--sel);
    opacity: 0.7;
  }
  .caret {
    position: absolute;
    top: 3px;
    left: 50%;
    transform: translateX(-50%);
    font-size: 8px;
    color: var(--text-3);
  }
  .scroller {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    position: relative;
  }
  .scroller.colflow {
    overflow-y: hidden;
    overflow-x: auto;
  }
  .grouphdr {
    position: absolute;
    left: 8px;
    right: 8px;
    top: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 8px;
    border-radius: 4px;
  }
  .grouphdr:hover {
    background: var(--hover);
  }
  .gchev {
    font-size: 10px;
    color: var(--text-3);
    width: 14px;
  }
  .ghlabel {
    font-size: 13px;
    font-weight: 600;
  }
  .ghcount {
    font-size: 13px;
    color: var(--text-3);
  }
  .canvas {
    position: relative;
    min-height: 100%;
  }
  .vrow {
    position: absolute;
    left: 8px;
    right: 8px;
    top: 0;
    align-items: center;
    border-radius: 4px;
    padding: 0 4px 0 8px;
  }
  .vrow:hover,
  .gcell:hover {
    background: var(--hover);
  }
  .vrow.selected,
  .gcell.selected {
    background: var(--sel);
  }
  .vrow.selected:hover,
  .gcell.selected:hover {
    background: var(--sel-hover);
  }
  .vrow.drophover,
  .gcell.drophover {
    background: var(--sel-hover);
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  .cell {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 8px;
    overflow: hidden;
    white-space: nowrap;
  }
  .cell.name {
    color: var(--text);
  }
  .cell.name.dim,
  .glabel.dim {
    opacity: 0.55;
  }
  .fname {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fileicon {
    flex-shrink: 0;
  }
  .placeholder {
    font-size: 16px;
    color: var(--text-3);
    text-align: center;
  }
  .cell.sub {
    color: var(--text-3);
    font-size: 13px;
  }
  .cell.size {
    justify-content: flex-end;
  }
  /* Grid cells */
  .gcell {
    position: absolute;
    left: 0;
    top: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 2px 8px;
    border-radius: 4px;
    overflow: hidden;
  }
  .gcell.vert {
    flex-direction: column;
    justify-content: flex-start;
    gap: 2px;
    padding-top: 8px;
    text-align: center;
  }
  .gicon {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .glabel {
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    line-height: 1.25;
  }
  .gcell.vert .glabel {
    width: 100%;
    align-items: center;
  }
  .gname {
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .gname.clamp2 {
    white-space: normal;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    word-break: break-word;
  }
  .gsub {
    font-size: 12px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rename {
    flex: 1;
    background: var(--menu-bg);
    border: 1px solid var(--accent);
    border-radius: 2px;
    padding: 2px 4px;
    user-select: text;
    min-width: 0;
    width: 100%;
    font-size: 13px;
  }
  .empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--text-3);
    font-size: 13px;
    pointer-events: none;
  }
  .marquee {
    position: absolute;
    border: 1px solid var(--accent);
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    pointer-events: none;
    z-index: 5;
  }
  .ctxmenu {
    min-width: 260px;
    max-width: 340px;
  }
  .emptyfolder {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    font-size: 13px;
    color: var(--text-3);
    pointer-events: none;
  }
  .scrollwrap {
    max-height: 60vh;
    overflow-y: auto;
    overflow-x: visible;
  }
  .menuload {
    padding: 10px 14px;
    font-size: 12px;
    color: var(--text-3);
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
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .subchev {
    font-size: 10px;
    color: var(--text-3);
  }
  .submenu {
    position: absolute;
    left: calc(100% - 4px);
    top: -4px;
    max-height: 50vh;
    overflow-y: auto;
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
