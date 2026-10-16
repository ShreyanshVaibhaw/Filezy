<script lang="ts">
  import { menuKeys, fitMenu } from "./menukeys";
  import { COLUMNS, colDef, columnsFor, saveColumns, SEARCH_KEY, attrLetters, type Col, type ColKey } from "./columns";
  import {
    app, rows, sel, ensureRange, requestMeta, requestProps, propOf, iconOf, typeOf, fmtDate, fmtSizeKB,
    openEntry, sortBy, hooks, refresh, selectAll, isSelected, clearSel, withSel, firstSel,
    refreshClipState, requestThumb, thumbOf, entryPath, collapsedGroups, loadNewItems, newItems,
    extMeta, isPinned, isCut, displayName, newTab, isHidden, can, up, folderName,
    requestStatus, statusOf, cloudStateOf, isKept, isFavorite, setFavorite,
  } from "./store.svelte";
  import {
    commitRename, copySel, paste, del, newFolder, newFile, startRename, openInNewTabs,
    openInNewWindow, openFileLocation, createShortcuts, pasteShortcuts, shareSel, propertiesSel, pinSelected,
  } from "./ops.svelte";
  import { selectRange, toggleRow, focusedRow, openSelection } from "./selection.svelte";
  import type { Entry } from "./store.svelte";
  import { settings, menuOn, anyMenuOn } from "./settings.svelte";
  import ShellItems from "./ShellItems.svelte";
  import { filterExtras, followMenu, OUR_VERBS, type ShellItem, type ShellMenu } from "./shellmenu";
  import { invoke } from "@tauri-apps/api/core";
  import { displayPath, isArchive, isArchiveName, isServer, isVirtual } from "./paths";
  import { compressSel, extractAll, extractSelected } from "./archive.svelte";

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

  // Loading feedback only when a folder takes long enough to notice.
  let slowLoad = $state(false);
  $effect(() => {
    if (!app.loading) {
      slowLoad = false;
      return;
    }
    const timer = setTimeout(() => (slowLoad = true), 300);
    return () => clearTimeout(timer);
  });

  let scroller: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let scrollLeft = $state(0);
  let viewH = $state(600);
  let gridW = $state(800);

  // Compact view tightens the row-based views (details/list), like Explorer's.
  const v = $derived.by(() => {
    const base = VIEWS[app.view] ?? VIEWS.details;
    if (!settings.compact) return base;
    if (app.view === "details") return { ...base, rowH: 28 };
    if (app.view === "list") return { ...base, rowH: 24 };
    return base;
  });
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

  // Grouped GRID views (icons/tiles): pixel-based segments - a header band per
  // group followed by its rows of cells.
  const HDR_H = 40;
  const ggrid = $derived.by(() => {
    if (!v.grid || v.flow === "column" || app.groupKey === "none" || !app.groups.length)
      return null;
    let y = 0;
    const segs = app.groups.map((g) => {
      const open = !collapsedGroups.has(g.label);
      const rowsN = open ? Math.ceil(g.count / cols) : 0;
      const headerTop = y;
      const cellsTop = headerTop + HDR_H;
      y = cellsTop + rowsN * v.rowH + (open ? 6 : 0);
      return { g, open, rowsN, headerTop, cellsTop };
    });
    return { segs, height: y };
  });

  // Grouped LIST view (column flow): groups laid out left-to-right, each with a
  // header on top and its items flowing top-to-bottom in columns beneath it.
  const glist = $derived.by(() => {
    if (!v.grid || v.flow !== "column" || app.groupKey === "none" || !app.groups.length)
      return null;
    const ipcG = Math.max(1, Math.floor((viewH - HDR_H) / v.rowH));
    let x = 0;
    const segs = app.groups.map((g) => {
      const open = !collapsedGroups.has(g.label);
      const colsN = open ? Math.ceil(g.count / ipcG) : 0;
      const xStart = x;
      x += Math.max(colsN, 1) * v.cellW + 24; // collapsed groups keep a header-wide slot
      return { g, open, colsN, xStart };
    });
    return { segs, width: x, ipcG };
  });

  function segOf<T extends { g: Group }>(segs: T[], i: number): T {
    let lo = 0,
      hi = segs.length - 1,
      k = 0;
    while (lo <= hi) {
      const m = (lo + hi) >> 1;
      if (segs[m].g.start <= i) {
        k = m;
        lo = m + 1;
      } else hi = m - 1;
    }
    return segs[k];
  }

  /** Absolute canvas position of entry i in a grid view (group-aware). */
  function cellPos(i: number): { x: number; y: number } {
    if (glist) {
      const s = segOf(glist.segs, i);
      const local = i - s.g.start;
      return {
        x: s.xStart + 8 + Math.floor(local / glist.ipcG) * v.cellW,
        y: HDR_H + (local % glist.ipcG) * v.rowH,
      };
    }
    if (ggrid) {
      const segs = ggrid.segs;
      let lo = 0,
        hi = segs.length - 1,
        k = 0;
      while (lo <= hi) {
        const m = (lo + hi) >> 1;
        if (segs[m].g.start <= i) {
          k = m;
          lo = m + 1;
        } else hi = m - 1;
      }
      const s = segs[k];
      const local = i - s.g.start;
      return {
        x: 8 + (local % cols) * v.cellW,
        y: s.cellsTop + Math.floor(local / cols) * v.rowH,
      };
    }
    const r = v.flow === "column" ? i % ipc : Math.floor(i / cols);
    const c = v.flow === "column" ? Math.floor(i / ipc) : i % cols;
    return { x: 8 + c * v.cellW, y: r * v.rowH };
  }

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
    if (glist) {
      const left = scrollLeft - 200;
      const right = scrollLeft + gridW + 200;
      for (const s of glist.segs) {
        if (!s.open || s.colsN === 0) continue;
        if (s.xStart + s.colsN * v.cellW < left || s.xStart > right) continue;
        const cF = Math.max(0, Math.floor((left - s.xStart) / v.cellW));
        const cL = Math.min(s.colsN - 1, Math.floor((right - s.xStart) / v.cellW));
        for (let c = cF; c <= cL; c++) {
          for (let r = 0; r < glist.ipcG; r++) {
            const i = s.g.start + c * glist.ipcG + r;
            if (i < s.g.start + s.g.count) out.push(i);
          }
        }
      }
      return out;
    }
    if (ggrid) {
      const OV = 200; // px overscan
      const top = scrollTop - OV;
      const bot = scrollTop + viewH + OV;
      for (const s of ggrid.segs) {
        if (!s.open || s.rowsN === 0) continue;
        if (s.cellsTop + s.rowsN * v.rowH < top || s.cellsTop > bot) continue;
        const rF = Math.max(0, Math.floor((top - s.cellsTop) / v.rowH));
        const rL = Math.min(s.rowsN - 1, Math.floor((bot - s.cellsTop) / v.rowH));
        for (let r = rF; r <= rL; r++) {
          for (let c = 0; c < cols; c++) {
            const i = s.g.start + r * cols + c;
            if (i < s.g.start + s.g.count) out.push(i);
          }
        }
      }
      return out;
    }
    for (let i = range.first; i < range.last; i++) out.push(i);
    return out;
  });

  // Entry-index span behind the visible display rows (identical when ungrouped).
  const entrySpan = $derived.by(() => {
    if (ggrid || glist) {
      return visible.length
        ? { first: visible[0], last: visible[visible.length - 1] + 1 }
        : { first: 0, last: 0 };
    }
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
      if (v.icon >= 32) requestThumb(e, v.icon);
      if (!v.grid) requestProps(e, propCols);
      if (!v.grid && statusCol) requestStatus(e);
    }
  });
  $effect(() => {
    hooks.gridCols = v.flow === "column" ? 1 : cols;
    // Left/Right move between items only where items sit side by side.
    hooks.hStep = v.flow === "column" ? (glist ? glist.ipcG : ipc) : v.grid ? 1 : 0;
    // Page Up/Down move by what fits on screen.
    hooks.pageSize = v.flow === "column"
      ? Math.max(1, Math.floor(gridW / v.cellW)) * ipc
      : Math.max(1, Math.floor(viewH / v.rowH) - 1) * cols;
  });

  /// Shift+F10 and the menu key: the item menu at the focused row, or the
  /// folder menu when nothing is selected.
  hooks.contextMenu = () => {
    dragged = false;
    const i = focusedRow();
    const el = i === undefined ? null : document.getElementById(`item-${i}`);
    const pane = scroller?.getBoundingClientRect();
    const r = el?.getBoundingClientRect();
    const at = r
      ? { clientX: r.left + 28, clientY: r.bottom - 2 } // just below the item, which stays visible
      : { clientX: (pane?.left ?? 0) + 40, clientY: (pane?.top ?? 0) + 40 };
    onContext({ ...at, preventDefault() {} }, i !== undefined && app.total ? i : null);
  };

  hooks.scrollTo = (px) => {
    if (scroller) {
      scroller.scrollTop = px;
      scroller.scrollLeft = 0;
    }
  };
  hooks.getScroll = () => scroller?.scrollTop ?? 0;
  hooks.visibleRange = () => entrySpan;
  hooks.visibleCount = () => (v.flow === "column" ? ipc * (Math.ceil(gridW / v.cellW) + 1) : (Math.ceil(viewH / v.rowH) + 6) * cols);
  hooks.scrollToIndex = (i) => {
    if (!scroller) return;
    if (glist) {
      const s = glist.segs.find(({ g }) => i >= g.start && i < g.start + g.count);
      if (!s) return;
      const left = s.open
        ? s.xStart + Math.floor((i - s.g.start) / glist.ipcG) * v.cellW
        : s.xStart;
      if (left < scroller.scrollLeft) scroller.scrollLeft = left;
      else if (left + v.cellW > scroller.scrollLeft + gridW)
        scroller.scrollLeft = left + v.cellW - gridW;
      return;
    }
    if (ggrid) {
      const s = ggrid.segs.find(({ g }) => i >= g.start && i < g.start + g.count);
      if (!s) return;
      const top = s.open ? s.cellsTop + Math.floor((i - s.g.start) / cols) * v.rowH : s.headerTop;
      const h = s.open ? v.rowH : HDR_H;
      if (top < scroller.scrollTop) scroller.scrollTop = top;
      else if (top + h > scroller.scrollTop + viewH) scroller.scrollTop = top + h - viewH;
      return;
    }
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
    // The list keeps the focus (rows are its active descendant), not the row.
    ev.preventDefault();
    scroller?.focus({ preventScroll: true });
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
      app.focus = i;
      return;
    }
    if (ev.shiftKey) {
      // Shift selects from the anchor; Ctrl+Shift adds that range.
      selectRange(app.anchor, i, ev.ctrlKey);
      app.focus = i;
    } else if (ev.ctrlKey && isSelected(i)) {
      // Deselect on release, unless this turns into a Ctrl+drag of the selection.
      toggleTo = i;
      app.focus = i;
    } else if (ev.ctrlKey) {
      toggleRow(i);
    } else if (isSelected(i) && selCountNow() > 1) {
      // Keep the selection for a possible drag; a plain release selects just this one.
      collapseTo = i;
      app.focus = i;
    } else {
      clearSel();
      sel.add(i);
      app.anchor = i;
      app.focus = i;
    }
  }

  // A click on one item of a multi-selection collapses to it on release, and a
  // Ctrl+click on a selected item deselects it on release, unless either became a drag.
  let collapseTo = -1;
  let toggleTo = -1;
  const selCountNow = () => (app.allSelected ? app.total : sel.size);
  function rowMouseUp() {
    if (toggleTo >= 0) toggleRow(toggleTo);
    toggleTo = -1;
    if (collapseTo < 0) return;
    clearSel();
    sel.add(collapseTo);
    app.anchor = collapseTo;
    collapseTo = -1;
  }

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
  ];
  // Inside an archive: look, open and extract; nothing in it changes yet.
  const inArchive = $derived(isArchive(app.path));
  const BLANK_IDS = ["blank.refresh", "blank.paste", "blank.pasteshortcut", "blank.new", "blank.selectall", "blank.terminal"];

  function onContext(ev: { clientX: number; clientY: number; shiftKey?: boolean; preventDefault(): void }, i: number | null) {
    ev.preventDefault();
    if (dragged) return void (dragged = false);
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
    ctxItems = null;
    ctxNote = "";
    const seq = ++ctxSeq;
    if (i !== null) {
      // One loaded row answers at once; larger selections ask the backend.
      const one = !app.allSelected && sel.size === 1 ? rows[i] : undefined;
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
    withSel((s) => invoke<ShellMenu>("shell_menu", { sel: s }))
      .then((m) => {
        if (seq !== ctxSeq || !ctx) return;
        ctxSession = m.session;
        ctxNote = m.note ?? "";
        ctxItems = filterExtras(m.items, OUR_VERBS);
        followMenu(m, () => seq === ctxSeq && !!ctx, (items) => (ctxItems = filterExtras(items, OUR_VERBS)));
      })
      .catch((e) => {
        if (seq !== ctxSeq) return;
        // A newer right-click took over the worker; its own result will come.
        if (String(e) !== "superseded") ctxNote = String(e);
        ctxItems = [];
      });
  }

  /// Run a shell verb Filezy implements itself. False when it isn't one.
  async function runVerb(v: string): Promise<boolean> {
    const first = firstSel();
    const fe = first === undefined ? undefined : rows[first];
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
    // Closing our menu hands the focus back to the page; let that land first,
    // or it arrives while Windows' menu is open and closes it.
    await new Promise((r) => setTimeout(r, 60));
    const s = window.devicePixelRatio || 1;
    try {
      const verb = await withSel((sel) =>
        invoke<string | null>("shell_menu_native", {
          sel,
          x: Math.round(at.x * s),
          y: Math.round(at.y * s),
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

  // Inline rename
  // The rename box is recreated whenever the list re-renders (a refresh moves
  // rows), so the typed text is kept here, tied to the item being renamed.
  let renameValue = $state("");
  let renameKey = "";
  function renameInput(node: HTMLInputElement, e: Entry) {
    const key = entryPath(e);
    node.focus();
    if (key === renameKey) return;
    renameKey = key;
    renameValue = e.name;
    node.value = e.name;
    // Like Explorer, select the name without its extension.
    const dot = e.name.lastIndexOf(".");
    node.setSelectionRange(0, !e.is_dir && dot > 0 ? dot : e.name.length);
  }
  $effect(() => {
    if (app.renaming < 0) renameKey = "";
  });
  /**
   * Blur commits, like Explorer. Chromium also blurs a box that is merely being
   * replaced when the list re-renders, so wait a tick: if a fresh box for the same
   * item has focus by then, the rename is still in progress.
   */
  function renameBlur() {
    const key = renameKey;
    setTimeout(() => {
      const active = document.activeElement;
      if (active instanceof HTMLInputElement && active.classList.contains("rename")) return;
      // The row is between listings (or scrolled away); its box comes back with the text.
      if (app.renaming >= 0 && !rows[app.renaming]) return;
      if (app.renaming >= 0 && renameKey === key) commitRename(app.renaming, renameValue);
    }, 0);
  }

  // Details columns: Name is fixed leftmost and flexes; the others are each
  // folder's own choice (columns.ts), resized, reordered and saved per folder.
  // Search results keep one layout of their own.
  const layoutKey = $derived(app.search ? SEARCH_KEY : app.path);
  let columns = $state<Col[]>([]);
  $effect(() => {
    columns = columnsFor(layoutKey, app.cloud);
  });
  const gridCols = $derived(`minmax(200px, 1fr) ${columns.map((c) => c.w + "px").join(" ")}`);
  const propCols = $derived(columns.filter((c) => colDef(c.k).prop).map((c) => c.k));
  const statusCol = $derived(columns.some((c) => c.k === "status"));

  /** Explorer's Status icons, by STORAGE_PROVIDER_STATE: a cloud when only
   *  online, checks when on this device, arrows while syncing. */
  function statusIcon(state: number): { glyph: string; tone: string } {
    switch (state) {
      case 1: return { glyph: "", tone: "cloud" };
      case 2: return { glyph: "", tone: "ok" };
      case 3: return { glyph: "", tone: "ok" };
      case 7: return { glyph: "", tone: "bad" };
      case 8: return { glyph: "", tone: "warn" };
      case 9: return { glyph: "", tone: "bad" };
      default: return { glyph: "", tone: "cloud" }; // pending, downloading, uploading
    }
  }
  function persistColumns() {
    if (layoutKey === SEARCH_KEY || !isVirtual(layoutKey)) saveColumns(layoutKey, columns);
  }

  function cellText(e: Entry, key: ColKey): string {
    switch (key) {
      case "modified":
        return fmtDate(e.modified);
      case "created":
        return fmtDate(e.created);
      case "type":
        return typeOf(e);
      case "size":
        return fmtSizeKB(e);
      case "location":
        return displayPath(e.parent ?? app.path);
      case "attributes":
        return attrLetters(e.attrs);
      case "status":
        return statusOf(e);
      default:
        return propOf(e, key);
    }
  }

  // Header menu: which columns show, and sizing them to their contents.
  let hdrMenu = $state<{ x: number; y: number; key: ColKey | null } | null>(null);
  function headerContext(ev: MouseEvent, key: ColKey | null) {
    ev.preventDefault();
    hdrMenu = { x: ev.clientX, y: ev.clientY, key };
  }
  function toggleColumn(k: ColKey) {
    columns = columns.some((c) => c.k === k) ? columns.filter((c) => c.k !== k) : [...columns, { k, w: colDef(k).width }];
    persistColumns();
  }

  let measurer: CanvasRenderingContext2D | null = null;
  /** Widest of the column's texts in the list's font, as Explorer sizes a column to fit. */
  function fitWidth(k: ColKey): number {
    measurer ??= document.createElement("canvas").getContext("2d");
    const cell = scroller?.querySelector(".cell.sub") ?? headerEl;
    if (!measurer || !cell) return colDef(k).width;
    measurer.font = getComputedStyle(cell).font;
    let widest = measurer.measureText(colDef(k).label).width + 16; // room for the sort caret
    let n = 0;
    for (const e of rows) {
      if (!e) continue;
      // Status also draws its 16px icon and a 6px gap.
      widest = Math.max(widest, measurer.measureText(cellText(e, k)).width + (k === "status" && statusOf(e) ? 22 : 0));
      if (++n >= 20_000) break;
    }
    return Math.min(800, Math.max(50, Math.ceil(widest) + 24));
  }
  function fitColumn(k: ColKey) {
    columns = columns.map((c) => (c.k === k ? { ...c, w: fitWidth(k) } : c));
    persistColumns();
  }
  function fitAll() {
    columns = columns.map((c) => ({ ...c, w: fitWidth(c.k) }));
    persistColumns();
  }

  let headerEl: HTMLDivElement | undefined = $state();

  // Column resize (drag the divider at a header's right edge; double-click sizes it to fit).
  let drag: { key: ColKey; startX: number; startW: number } | null = null;
  function startResize(key: ColKey, ev: MouseEvent) {
    ev.stopPropagation();
    ev.preventDefault();
    drag = { key, startX: ev.clientX, startW: columns.find((c) => c.k === key)?.w ?? 100 };
  }
  function onResizeMove(ev: MouseEvent) {
    if (!drag) return;
    const d = drag;
    const w = Math.max(50, d.startW + ev.clientX - d.startX);
    columns = columns.map((c) => (c.k === d.key ? { ...c, w } : c));
  }
  function endResize() {
    if (drag) persistColumns();
    drag = null;
  }

  // Column reorder (drag a metadata header onto another; live-swaps as you move).
  let colDrag = $state<{ key: ColKey; startX: number; moved: boolean } | null>(null);
  let suppressSort = false;
  function startColDrag(key: ColKey, ev: MouseEvent) {
    if (ev.button !== 0) return;
    colDrag = { key, startX: ev.clientX, moved: false };
  }
  function colDragMove(ev: MouseEvent) {
    if (!colDrag || !headerEl) return;
    if (!colDrag.moved && Math.abs(ev.clientX - colDrag.startX) < 5) return;
    colDrag.moved = true;
    const cells = [...headerEl.querySelectorAll<HTMLElement>(".hcell[data-meta]")];
    const from = columns.findIndex((c) => c.k === colDrag!.key);
    let to = columns.length - 1;
    for (let idx = 0; idx < cells.length; idx++) {
      const r = cells[idx].getBoundingClientRect();
      if (ev.clientX < r.left + r.width / 2) {
        to = idx;
        break;
      }
    }
    if (to !== from) {
      const moving = columns[from];
      const next = columns.filter((c) => c !== moving);
      next.splice(to, 0, moving);
      columns = next;
    }
  }
  function endColDrag() {
    if (colDrag?.moved) {
      suppressSort = true;
      persistColumns();
    }
    colDrag = null;
  }
  function headerClick(key: ColKey | "name") {
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
    } else if (glist) {
      const rFirst = Math.max(0, Math.floor((m.y - HDR_H) / v.rowH));
      const rLast = Math.min(glist.ipcG - 1, Math.floor((m.y + m.h - HDR_H) / v.rowH));
      for (const s of glist.segs) {
        if (!s.open || s.colsN === 0) continue;
        const cFirst = Math.max(0, Math.floor((m.x - s.xStart - 8) / v.cellW));
        const cLast = Math.min(s.colsN - 1, Math.floor((m.x + m.w - s.xStart - 8) / v.cellW));
        if (cLast < 0 || cFirst > s.colsN - 1) continue;
        for (let c = cFirst; c <= cLast; c++) {
          for (let r = rFirst; r <= rLast; r++) {
            const i = s.g.start + c * glist.ipcG + r;
            if (i < s.g.start + s.g.count) sel.add(i);
          }
        }
      }
    } else if (ggrid) {
      const cFirst = Math.max(0, Math.floor((m.x - 8) / v.cellW));
      const cLast = Math.min(cols - 1, Math.floor((m.x + m.w - 8) / v.cellW));
      for (const s of ggrid.segs) {
        if (!s.open || s.rowsN === 0) continue;
        const rFirst = Math.max(0, Math.floor((m.y - s.cellsTop) / v.rowH));
        const rLast = Math.min(s.rowsN - 1, Math.floor((m.y + m.h - s.cellsTop) / v.rowH));
        for (let r = rFirst; r <= rLast; r++) {
          for (let c = cFirst; c <= cLast; c++) {
            const i = s.g.start + r * cols + c;
            if (i < s.g.start + s.g.count) sel.add(i);
          }
        }
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
  // Either button starts a drag; the shell's drop source then follows that
  // button, and Ctrl or Shift held at any point picks copy or move. Drops come
  // in through drag.ts.
  let dragArm: { x: number; y: number } | null = null;
  // A right-drag just ended: its button release must not open the item menu.
  let dragged = false;
  function armDrag(ev: MouseEvent) {
    dragged = false;
    if (ev.button === 0 || ev.button === 2) dragArm = { x: ev.clientX, y: ev.clientY };
  }
  function dragMove(ev: MouseEvent) {
    if (!dragArm) return;
    if (Math.abs(ev.clientX - dragArm.x) + Math.abs(ev.clientY - dragArm.y) > 6) {
      dragArm = null;
      collapseTo = -1;
      toggleTo = -1;
      dragged = true;
      // ponytail: an archive's entries can't be dragged out yet (Extract does it).
      if (!inArchive) withSel((sel) => invoke("start_drag", { sel })).catch((e) => (app.status = String(e)));
    }
  }

  // Where a drop on the list background goes: the folder shown, if it is one.
  const backgroundDrop = $derived(!app.search && !isVirtual(app.path) && !isArchive(app.path) ? app.path : undefined);
</script>

<svelte:window
  onmousedown={(e) => {
    if (!(e.target as HTMLElement).closest(".menu")) {
      ctx = null;
      hdrMenu = null;
    }
  }}
  onmousemove={(e) => { onResizeMove(e); dragMove(e); colDragMove(e); marqueeMove(e); }}
  onmouseup={() => { endResize(); dragArm = null; endColDrag(); marqueeEnd(); rowMouseUp(); }}
/>

<div class="pane" data-filelist>
  {#if !v.grid}
    <div class="header" bind:this={headerEl} style="height:{HEADER}px; grid-template-columns:{gridCols}">
      <button class="hcell" onclick={() => headerClick("name")} oncontextmenu={(ev) => headerContext(ev, null)}>
        {#if app.sortKey === "name"}
          <span class="glyph caret">{app.asc ? "" : ""}</span>
        {/if}
        <span class="hlabel">Name</span>
      </button>
      {#each columns as c (c.k)}
        <button
          class="hcell"
          class:right={colDef(c.k).right}
          class:coldragging={colDrag?.key === c.k && colDrag.moved}
          data-meta={c.k}
          onmousedown={(ev) => startColDrag(c.k, ev)}
          onclick={() => headerClick(c.k)}
          oncontextmenu={(ev) => headerContext(ev, c.k)}
        >
          {#if app.sortKey === c.k}
            <span class="glyph caret">{app.asc ? "" : ""}</span>
          {/if}
          <span class="hlabel">{colDef(c.k).label}</span>
          <div class="hresize" role="none" onmousedown={(ev) => startResize(c.k, ev)} ondblclick={(ev) => { ev.stopPropagation(); fitColumn(c.k); }}></div>
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
    data-drop={backgroundDrop}
    data-drop-label={backgroundDrop ? folderName(backgroundDrop) : undefined}
    data-drop-quiet
    data-drop-scroll
    role="listbox"
    tabindex="0"
    aria-multiselectable="true"
    aria-label={folderName(app.search?.root ?? app.path)}
    aria-activedescendant={app.focus >= entrySpan.first && app.focus < entrySpan.last ? `item-${app.focus}` : undefined}
  >
    <div
      class="canvas"
      style={ggrid
        ? `height:${ggrid.height}px`
        : glist
          ? `height:${glist.ipcG * v.rowH + HDR_H}px; width:${glist.width + 16}px`
          : v.flow === "column"
            ? `height:${ipc * v.rowH}px; width:${cols * v.cellW + 16}px`
            : `height:${totalRows * v.rowH}px`}
    >
      {#if glist}
        {#each glist.segs as s (s.g.label)}
          {#if s.xStart + Math.max(s.colsN, 1) * v.cellW > scrollLeft - 200 && s.xStart < scrollLeft + gridW + 200}
            <div
              class="grouphdr listhdr"
              style="transform: translateX({s.xStart + 8}px); width:{Math.max(s.colsN, 1) * v.cellW - 16}px; height:{HDR_H}px"
              role="button"
              tabindex="-1"
              onmousedown={() => toggleGroup(s.g.label)}
            >
              <span class="glyph gchev">{collapsedGroups.has(s.g.label) ? "" : ""}</span>
              <span class="ghlabel">{s.g.label}</span>
              <span class="ghcount">({s.g.count})</span>
            </div>
          {/if}
        {/each}
      {/if}
      {#if ggrid}
        {#each ggrid.segs as s (s.g.label)}
          {#if s.headerTop + HDR_H > scrollTop - 200 && s.headerTop < scrollTop + viewH + 200}
            <div
              class="grouphdr"
              style="transform: translateY({s.headerTop}px); height:{HDR_H}px"
              role="button"
              tabindex="-1"
              onmousedown={() => toggleGroup(s.g.label)}
            >
              <span class="glyph gchev">{collapsedGroups.has(s.g.label) ? "" : ""}</span>
              <span class="ghlabel">{s.g.label}</span>
              <span class="ghcount">({s.g.count})</span>
            </div>
          {/if}
        {/each}
      {/if}
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
            class:focused={app.focus === i}
            class:cut={!!e && isCut(e)}
            id={`item-${i}`}
            title={e?.parent ?? ""}
            data-idx={i}
            data-drop={e?.is_dir ? entryPath(e) : undefined}
            data-drop-label={e?.is_dir ? displayName(e) : undefined}
            style="transform: translateY({d * v.rowH}px); height:{v.rowH}px; grid-template-columns:{gridCols}"
            onmousedown={(ev) => { if (e) { rowMouseDown(i, ev); armDrag(ev); } }}
            ondblclick={() => e && app.renaming !== i && openEntry(i)}
            oncontextmenu={(ev) => e && onContext(ev, i)}
            role="option"
            aria-selected={isSelected(i)}
            aria-setsize={app.total}
            aria-posinset={i + 1}
            tabindex="-1"
          >
            {#if e}
              <div class="cell name" class:dim={isHidden(e)}>
                {#if settings.checkboxes}
                  <span class="ck" class:on={isSelected(i)} aria-hidden="true" onmousedown={(ev) => { ev.stopPropagation(); if (ev.button === 0) toggleRow(i); }}>
                    <span class="glyph">&#xE73E;</span>
                  </span>
                {/if}
                {#if iconOf(e)}
                  <img class="fileicon" src={iconOf(e)} alt="" width="16" height="16" />
                {:else}
                  <span class="glyph fileicon placeholder">{e.is_dir ? "" : ""}</span>
                {/if}
                {#if app.renaming === i}
                  <input
                    class="rename"
                    use:renameInput={e}
                    bind:value={renameValue}
                    onkeydown={(ev) => {
                      ev.stopPropagation();
                      if (ev.key === "Enter") commitRename(i, renameValue);
                      else if (ev.key === "Escape") app.renaming = -1;
                      if (ev.key === "Enter" || ev.key === "Escape") scroller?.focus({ preventScroll: true });
                    }}
                    onblur={renameBlur}
                    onmousedown={(ev) => ev.stopPropagation()}
                    ondblclick={(ev) => ev.stopPropagation()}
                    spellcheck="false"
                  />
                {:else}
                  <span class="fname">{displayName(e)}</span>
                {/if}
              </div>
              {#each columns as c (c.k)}
                {@const cs = c.k === "status" ? cloudStateOf(e) : null}
                {#if cs}
                  {@const icon = statusIcon(cs.state)}
                  <div class="cell sub status" title={cs.text}>
                    <span class="glyph {icon.tone}" aria-hidden="true">{icon.glyph}</span>{cs.text}
                  </div>
                {:else}
                  <div class="cell sub" class:size={colDef(c.k).right}>{cellText(e, c.k)}</div>
                {/if}
              {/each}
            {/if}
          </div>
        {:else}
          {@const pos = cellPos(i)}
          <div
            class="gcell"
            class:vert={v.vertical}
            class:tile={v.tile}
            class:selected={isSelected(i)}
            class:focused={app.focus === i}
            class:cut={!!e && isCut(e)}
            id={`item-${i}`}
            data-idx={i}
            data-drop={e?.is_dir ? entryPath(e) : undefined}
            data-drop-label={e?.is_dir ? displayName(e) : undefined}
            style="transform: translate({pos.x}px, {pos.y}px); width:{v.cellW - 6}px; height:{v.rowH - 6}px"
            onmousedown={(ev) => { if (e) { rowMouseDown(i, ev); armDrag(ev); } }}
            ondblclick={() => e && app.renaming !== i && openEntry(i)}
            oncontextmenu={(ev) => e && onContext(ev, i)}
            role="option"
            aria-selected={isSelected(i)}
            aria-setsize={app.total}
            aria-posinset={i + 1}
            tabindex="-1"
          >
            {#if e}
              {@const src = v.icon >= 32 ? thumbOf(e, v.icon) : iconOf(e)}
              {#if settings.checkboxes}
                <span class="ck corner" class:on={isSelected(i)} aria-hidden="true" onmousedown={(ev) => { ev.stopPropagation(); if (ev.button === 0) toggleRow(i); }}>
                  <span class="glyph">&#xE73E;</span>
                </span>
              {/if}
              <div class="gicon" style={v.vertical ? `height:${v.icon + 8}px` : `width:${v.icon}px`}>
                {#if src}
                  <img src={src} alt="" style="max-width:{v.icon}px; max-height:{v.icon}px" />
                {:else}
                  <span class="glyph placeholder" style="font-size:{Math.max(16, v.icon * 0.7)}px">
                    {e.is_dir ? "" : ""}
                  </span>
                {/if}
              </div>
              <div class="glabel" class:dim={isHidden(e)}>
                {#if app.renaming === i}
                  <input
                    class="rename"
                    use:renameInput={e}
                    bind:value={renameValue}
                    onkeydown={(ev) => {
                      ev.stopPropagation();
                      if (ev.key === "Enter") commitRename(i, renameValue);
                      else if (ev.key === "Escape") app.renaming = -1;
                      if (ev.key === "Enter" || ev.key === "Escape") scroller?.focus({ preventScroll: true });
                    }}
                    onblur={renameBlur}
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
    {#if app.error}
      <div class="state error" role="alert">
        <span class="glyph sicon">&#xE783;</span>
        <div class="stext">{app.error}</div>
        <div class="sbtns">
          <button class="sbtn" onclick={() => refresh()}>Try again</button>
          <button class="sbtn" onclick={up}>Go up</button>
        </div>
      </div>
    {:else if app.total === 0 && !app.loading}
      <div class="state" role="status">
        {#if app.search}
          {app.searching
            ? "Searching…"
            : app.search.stopped
              ? "Nothing was found before the search was stopped."
              : "No items match your search."}
        {:else if isServer(app.path)}
          This computer doesn't share any folders, or none you can see.
        {:else}
          This folder is empty.
        {/if}
      </div>
    {/if}
  </div>
  {#if slowLoad}
    <div class="loadbar" role="progressbar" aria-label="Loading folder"><span></span></div>
  {/if}
</div>

{#if hdrMenu}
  <div class="menu hdrmenu" style="left:{hdrMenu.x}px; top:{hdrMenu.y}px" use:menuKeys={() => (hdrMenu = null)} use:fitMenu={hdrMenu}>
    {#if hdrMenu.key}
      <button class="menu-item" onclick={() => { const k = hdrMenu?.key; hdrMenu = null; if (k) fitColumn(k); }}>
        <span class="glyph"></span> Size column to fit
      </button>
    {/if}
    <button class="menu-item" onclick={() => { hdrMenu = null; fitAll(); }}>
      <span class="glyph"></span> Size all columns to fit
    </button>
    <div class="menu-sep"></div>
    <button class="menu-item" role="menuitemcheckbox" aria-checked="true" disabled>
      <span class="glyph">&#xE73E;</span> Name
    </button>
    {#each COLUMNS as d (d.key)}
      {@const on = columns.some((c) => c.k === d.key)}
      <button class="menu-item" role="menuitemcheckbox" aria-checked={on} onclick={() => { hdrMenu = null; toggleColumn(d.key); }}>
        <span class="glyph">{on ? "\uE73E" : ""}</span> {d.label}
      </button>
    {/each}
  </div>
{/if}

{#if ctx}
  <div class="menu ctxmenu" style="left:{ctx.x}px; top:{ctx.y}px" use:menuKeys={() => (ctx = null)} use:fitMenu={ctx}>
    {#if ctx.onRow && anyMenuOn(STRIP_IDS) && !inArchive}
      <div class="strip">
        {#if menuOn("file.cut")}
          <button title="Cut" aria-label="Cut" onclick={() => { ctx = null; copySel(true); }}><span class="glyph">&#xE8C6;</span></button>
        {/if}
        {#if menuOn("file.copy")}
          <button title="Copy" aria-label="Copy" onclick={() => { ctx = null; copySel(false); }}><span class="glyph">&#xE8C8;</span></button>
        {/if}
        {#if menuOn("file.rename")}
          <button title="Rename" aria-label="Rename" onclick={() => { ctx = null; startRename(); }}><span class="glyph">&#xE8AC;</span></button>
        {/if}
        {#if menuOn("file.delete")}
          <button title="Delete" aria-label="Delete" onclick={() => { ctx = null; del(); }}><span class="glyph">&#xE74D;</span></button>
        {/if}
      </div>
      <div class="menu-sep"></div>
    {/if}
    {#if ctx.onRow}
      {@const fi = firstSel()}
      {@const fe = fi !== undefined ? rows[fi] : undefined}
      {@const multi = app.allSelected ? app.total : sel.size}
      {#if menuOn("file.open")}
        <button class="menu-item" onclick={() => { ctx = null; if (fi !== undefined) openEntry(fi); }}>
          <span class="glyph">&#xE8E5;</span> Open
        </button>
      {/if}
      {#if fe?.is_dir && menuOn("file.opennewtab")}
        <button class="menu-item" onclick={() => { ctx = null; openInNewTabs(); }}>
          <span class="glyph">&#xE8A7;</span> Open in new tab
        </button>
      {/if}
      {#if fe?.is_dir && multi === 1 && menuOn("file.opennewwindow")}
        <button class="menu-item" onclick={() => { ctx = null; openInNewWindow(); }}>
          <span class="glyph">&#xE737;</span> Open in new window
        </button>
      {/if}
      {#if fe && app.search && multi === 1 && menuOn("file.openlocation")}
        <button class="menu-item" onclick={() => { ctx = null; openFileLocation(entryPath(fe)); }}>
          <span class="glyph">&#xE838;</span> Open file location
        </button>
      {/if}
      {#if fe && !fe.is_dir && multi === 1 && menuOn("file.openwith") && !inArchive}
        <button class="menu-item" onclick={() => { ctx = null; invoke("open_with", { path: entryPath(fe) }).catch((e) => (app.status = String(e))); }}>
          <span class="glyph">&#xE7AC;</span> Open with
        </button>
      {/if}
      {#if inArchive && menuOn("file.extractall")}
        <button class="menu-item" onclick={() => { ctx = null; extractSelected(); }}>
          <span class="glyph">&#xE8B7;</span> Extract...
        </button>
        <button class="menu-item" onclick={() => { ctx = null; extractAll(); }}>
          <span class="glyph">&#xE8B7;</span> Extract all...
        </button>
      {:else if fe && !fe.is_dir && multi === 1 && isArchiveName(fe.name) && menuOn("file.extractall")}
        <button class="menu-item" onclick={() => { ctx = null; extractAll(entryPath(fe)); }}>
          <span class="glyph">&#xE8B7;</span> Extract all...
        </button>
      {/if}
      {#if app.cloud && !inArchive && menuOn("file.keep")}
        {@const kept = !!fe && isKept(fe)}
        <button class="menu-item" role="menuitemcheckbox" aria-checked={kept} onclick={() => { ctx = null; cloudPin(true); }}>
          <span class="glyph">{kept ? "" : ""}</span> Always keep on this device
        </button>
      {/if}
      {#if app.cloud && !inArchive && menuOn("file.freeup")}
        <button class="menu-item" onclick={() => { ctx = null; cloudPin(false); }}>
          <span class="glyph">&#xE753;</span> Free up space
        </button>
      {/if}
      {#if fe?.is_dir && menuOn("file.pin") && !inArchive}
        {#if isPinned(entryPath(fe))}
          <button class="menu-item" onclick={() => { ctx = null; pinSelected(false); }}>
            <span class="glyph">&#xE77A;</span> Unpin from Quick access
          </button>
        {:else}
          <button class="menu-item" onclick={() => { ctx = null; pinSelected(true); }}>
            <span class="glyph">&#xE840;</span> Pin to Quick access
          </button>
        {/if}
      {/if}
      {#if ctxFilesOnly && menuOn("file.favorite") && !inArchive}
        {@const fav = !!fe && isFavorite(entryPath(fe))}
        <button class="menu-item" onclick={() => { const on = !fav; ctx = null; favoriteSel(on); }}>
          <span class="glyph">{fav ? "" : ""}</span> {fav ? "Remove from Favorites" : "Add to Favorites"}
        </button>
      {/if}
      {#if menuOn("file.compress") && !inArchive}
        <button class="menu-item" onclick={() => { ctx = null; compressSel(); }}>
          <span class="glyph">&#xF012;</span> Compress to ZIP file
        </button>
      {/if}
      {#if menuOn("file.copypath") && !inArchive}
        <button class="menu-item" onclick={() => { ctx = null; withSel((sel) => invoke("copy_as_path", { sel })).then(refreshClipState).catch((e) => (app.status = String(e))); }}>
          <span class="glyph">&#xE8C8;</span> Copy as path
        </button>
      {/if}
      {#if menuOn("file.shortcut") && !inArchive}
        <button class="menu-item" onclick={() => { ctx = null; createShortcuts(); }}>
          <span class="glyph">&#xE71B;</span> Create shortcut
        </button>
      {/if}
      {#if ctxFilesOnly && menuOn("file.share") && !inArchive}
        <button class="menu-item" onclick={() => { ctx = null; shareSel(); }}>
          <span class="glyph">&#xE72D;</span> Share
        </button>
      {/if}
      {#if menuOn("file.properties") && !inArchive}
        <button class="menu-item" onclick={() => { ctx = null; propertiesSel(); }}>
          <span class="glyph">&#xE90F;</span> Properties
        </button>
      {/if}
      {#if ctxItems === null}
        <div class="menu-sep"></div>
        <div class="menuload">Loading…</div>
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
          <span class="glyph">&#xE712;</span> Show more options
        </button>
      {/if}
    {:else}
      {#if menuOn("blank.refresh")}
        <button class="menu-item" onclick={() => { ctx = null; refresh(); }}>
          <span class="glyph">&#xE72C;</span> Refresh
        </button>
      {/if}
      {#if menuOn("blank.paste") && !inArchive}
        <button class="menu-item" disabled={!app.canPaste} onclick={() => { ctx = null; paste(); }}>
          <span class="glyph">&#xE77F;</span> Paste
        </button>
      {/if}
      {#if menuOn("blank.pasteshortcut") && !inArchive}
        <button class="menu-item" disabled={!app.canPaste} onclick={() => { ctx = null; pasteShortcuts(); }}>
          <span class="glyph">&#xE71B;</span> Paste shortcut
        </button>
      {/if}
      {#if menuOn("blank.new") && !inArchive}
        <div class="menu-sep"></div>
        <div class="subwrap" role="none" onmouseenter={() => (blankNewOpen = true)} onmouseleave={() => (blankNewOpen = false)}>
          <button class="menu-item wide">
            <span class="glyph">&#xE710;</span>
            <span class="mtext">New</span>
            <span class="glyph subchev">&#xE76C;</span>
          </button>
          {#if blankNewOpen}
            <div class="menu submenu" use:menuKeys={() => (blankNewOpen = false)}>
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
      {/if}
      {#if menuOn("blank.selectall")}
        <button class="menu-item" onclick={() => { ctx = null; selectAll(); }}>
          <span class="glyph">&#xE8B3;</span> Select all
        </button>
      {/if}
      {#if inArchive && menuOn("file.extractall")}
        <div class="menu-sep"></div>
        <button class="menu-item" onclick={() => { ctx = null; extractAll(); }}>
          <span class="glyph">&#xE8B7;</span> Extract all...
        </button>
      {/if}
      {#if menuOn("blank.terminal") && !inArchive}
        <div class="menu-sep"></div>
        <button class="menu-item" onclick={() => { ctx = null; invoke("open_terminal", { dir: app.path }).catch((e) => (app.status = String(e))); }}>
          <span class="glyph">&#xE756;</span> Open in Terminal
        </button>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .pane {
    position: relative;
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
  .grouphdr.listhdr {
    left: 0;
    right: auto;
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
  /* Explorer's focus rectangle: only once the keyboard is in use. */
  .scroller:focus-visible {
    outline: none;
  }
  .scroller:focus-visible .focused {
    outline: 1px solid var(--text-2);
    outline-offset: -1px;
  }
  /* Item check boxes: on hover and on selected items. */
  .ck {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    display: grid;
    place-items: center;
    border: 1px solid var(--text-3);
    border-radius: 3px;
    /* Transparent rather than hidden, so the spot still takes a click before
       hover has painted it. */
    opacity: 0;
  }
  .ck .glyph {
    font-size: 10px;
    visibility: hidden;
  }
  .vrow:hover .ck,
  .gcell:hover .ck,
  .ck.on {
    opacity: 1;
  }
  .ck.on {
    background: var(--accent);
    border-color: var(--accent);
  }
  .ck.on .glyph {
    visibility: visible;
    color: var(--menu-bg);
  }
  .ck.corner {
    position: absolute;
    top: 4px;
    left: 4px;
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
  /* Cut items keep their name but ghost their icon until pasted, as in Explorer. */
  .vrow.cut .fileicon,
  .gcell.cut .gicon {
    opacity: 0.5;
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
  /* Plain text cells end in an ellipsis when the column is too narrow. */
  .cell.sub {
    display: block;
    text-overflow: ellipsis;
    color: var(--text-3);
    font-size: 13px;
  }
  .cell.size {
    text-align: right;
  }
  /* Status: Explorer's icon, then its wording. Fluent's status colors. */
  .cell.status .glyph {
    width: 16px;
    margin-right: 6px;
    font-size: 14px;
    vertical-align: -2px;
  }
  .cell.status .cloud {
    color: var(--accent);
  }
  .cell.status .ok {
    color: light-dark(#0f7b0f, #6ccb5f);
  }
  .cell.status .warn {
    color: light-dark(#9d5d00, #fce100);
  }
  .cell.status .bad {
    color: light-dark(#c42b1c, #ff99a4);
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
    line-clamp: 2;
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
  .state {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 24px;
    font-size: 13px;
    color: var(--text-3);
    text-align: center;
    pointer-events: none;
  }
  .state.error {
    pointer-events: auto;
    color: var(--text-2);
  }
  .sicon {
    font-size: 28px;
    color: var(--text-3);
  }
  .stext {
    max-width: 520px;
    user-select: text;
  }
  .sbtns {
    display: flex;
    gap: 8px;
  }
  .sbtn {
    height: 30px;
    padding: 0 14px;
    font-size: 13px;
    border: 1px solid var(--stroke-strong);
    border-radius: 4px;
  }
  .sbtn:hover {
    background: var(--hover);
  }
  /* Explorer's thin loading strip, shown only for slow folders. */
  .loadbar {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 2px;
    overflow: hidden;
    pointer-events: none;
  }
  .loadbar span {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 30%;
    background: var(--accent);
    animation: loadslide 1.1s ease-in-out infinite;
  }
  @keyframes loadslide {
    from {
      left: -30%;
    }
    to {
      left: 100%;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .loadbar span {
      animation: none;
      left: 0;
      width: 100%;
      opacity: 0.6;
    }
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
  @media (forced-colors: active) {
    .vrow.selected,
    .gcell.selected {
      forced-color-adjust: none;
      background: Highlight;
      color: HighlightText;
    }
    .vrow.selected *,
    .gcell.selected * {
      color: HighlightText;
    }
  }
</style>
