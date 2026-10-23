// Selection and keyboard focus in the file list, in Explorer's model: a focused
// item (where the keyboard is), an anchor (where a Shift range starts), and the
// selected set.
import { invoke } from "@tauri-apps/api/core";
import {
  app, sel, clearSel, hooks, rowAt, selCount, firstSel, withSel, addTabs, openEntry, openPath,
} from "./store.svelte";
import { confirm } from "./ask.svelte";

/// Put the focus on row `i` the way a key with these modifiers does:
/// plain selects only that row, Shift selects from the anchor to it (adding to
/// the selection with Ctrl), and Ctrl alone moves the focus without selecting.
export function moveFocus(i: number, mods: { shift?: boolean; ctrl?: boolean } = {}) {
  if (!app.total) return;
  i = Math.max(0, Math.min(app.total - 1, i));
  app.focus = i;
  if (mods.shift) selectRange(app.anchor, i, !!mods.ctrl);
  else if (!mods.ctrl) {
    clearSel();
    sel.add(i);
    app.anchor = i;
  }
  hooks.scrollToIndex?.(i);
}

/// Select rows a..b (either order). Additive keeps what was already selected.
export function selectRange(a: number, b: number, additive = false) {
  const [lo, hi] = [Math.min(a, b), Math.max(a, b)];
  if (!additive) {
    sel.clear();
    app.allSelected = false;
  }
  if (lo === 0 && hi === app.total - 1) {
    // ponytail: a full-range selection becomes the select-all flag, not 100k Set inserts
    sel.clear();
    app.allSelected = true;
    return;
  }
  if (app.allSelected) return;
  for (let k = lo; k <= hi; k++) sel.add(k);
}

/// Ctrl+Space (and a checkbox click): flip one row in or out of the selection.
export function toggleRow(i: number) {
  if (app.allSelected) {
    // Expand the flag so one row can leave it.
    app.allSelected = false;
    for (let k = 0; k < app.total; k++) if (k !== i) sel.add(k);
  } else if (sel.has(i)) sel.delete(i);
  else sel.add(i);
  app.anchor = i;
  app.focus = i;
}

export function selectNone() {
  clearSel();
}

/// Explorer's Invert selection.
export function invertSelection() {
  if (app.allSelected || sel.size === app.total) {
    clearSel();
    return;
  }
  if (!sel.size) {
    app.allSelected = true;
    return;
  }
  const was = new Set(sel);
  sel.clear();
  for (let k = 0; k < app.total; k++) if (!was.has(k)) sel.add(k);
}

// ---- type-to-select ----

let typed = "";
let typedAt = 0;

/// Typing jumps to the item whose name starts with what was typed in the last
/// second. Pressing one letter again steps to the next item starting with it.
export async function typeToSelect(ch: string) {
  const now = Date.now();
  typed = now - typedAt > 1000 ? ch : typed + ch;
  typedAt = now;
  const lower = typed.toLowerCase();
  const repeat = [...lower].every((c) => c === lower[0]);
  const prefix = repeat ? lower[0] : lower;
  // A new letter or a repeated one moves past the current item; a longer
  // prefix may still match the item already focused.
  const from = typed.length === 1 || repeat ? app.focus + 1 : Math.max(app.focus, 0);
  const rev = app.rev;
  const i = await invoke<number | null>("find_prefix", { rev, prefix, from: Math.max(0, from) % Math.max(1, app.total) }).catch(
    () => null,
  );
  if (i !== null && rev === app.rev) moveFocus(i);
}

// ---- open ----

/// Enter: open everything selected. One folder opens here; several folders open
/// in new tabs; files open in their apps. Many items ask first, like Explorer.
export async function openSelection() {
  const n = selCount();
  if (!n) return;
  if (n === 1) {
    const i = firstSel();
    if (i !== undefined && rowAt(i)) return openEntry(i);
  }
  if (n > 15 && !(await confirm("Open", `Are you sure you want to open ${n.toLocaleString()} items?`, { ok: "Open" }))) {
    return;
  }
  const items = await withSel((s) => invoke<{ path: string; is_dir: boolean }[]>("sel_items", { sel: s })).catch(
    (e) => {
      app.status = String(e);
      return [];
    },
  );
  for (const it of items) if (!it.is_dir) await openPath(it.path, true);
  await addTabs(items.filter((it) => it.is_dir).map((it) => it.path));
}

/** The row the keyboard is on, or the first selected one. */
export function focusedRow(): number | undefined {
  if (app.focus >= 0 && app.focus < app.total) return app.focus;
  return firstSel();
}

