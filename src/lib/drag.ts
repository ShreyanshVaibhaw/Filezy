// Drags over the window, from other apps or from Filezy itself.
//
// The native drop target (src-tauri/src/droptarget.rs) handles the OLE side:
// it sends every pointer position as a "drag" event, and decides the effect and
// the caption from the destination this module reports back for that point.
// Elements opt in with attributes:
//   data-drop="<folder>"     a drop here goes into that folder; "::RecycleBin" recycles
//   data-drop-label="<name>" the name in the caption "Move to <name>"
//   data-drop-quiet          a destination that isn't highlighted (the list background)
//   data-drop-hover          gets a "drophover" event after a short hover (switch tab, expand)
//   data-drop-scroll         scrolls while the pointer is near its top or bottom edge

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { baseName } from "./paths";
import { dropOn, type DropOp } from "./ops.svelte";

type Msg =
  | { kind: "enter" }
  | { kind: "over"; x: number; y: number }
  | { kind: "leave" }
  | { kind: "drop"; paths: string[]; dest: string; op: DropOp };

const HOVER_MS = 700;
/** Distance from a scroll container's edge where it starts scrolling, in CSS px. */
const EDGE = 28;

let lit: HTMLElement | null = null;
let sent = "";
let hoverEl: HTMLElement | null = null;
let hoverTimer = 0;
let pointer = { x: 0, y: 0 };
let edge: { box: HTMLElement; step: number } | null = null;
let edgeTimer = 0;

function light(el: HTMLElement | null) {
  const want = el && !("dropQuiet" in el.dataset) ? el : null;
  if (lit === want) return;
  lit?.removeAttribute("data-drop-on");
  lit = want;
  lit?.setAttribute("data-drop-on", "");
}

/** Tell the target where a drop at the pointer would go, when that changes. */
function report(el: HTMLElement | null) {
  const path = el?.dataset.drop ?? "";
  const label = path ? el?.dataset.dropLabel || baseName(path) || path : "";
  const key = `${path}\n${label}`;
  if (key === sent) return;
  sent = key;
  invoke("drop_dest", { dest: path ? { path, label } : null }).catch(() => {});
}

function hover(el: HTMLElement | null) {
  if (el === hoverEl) return;
  clearTimeout(hoverTimer);
  hoverEl = el;
  if (el) hoverTimer = window.setTimeout(() => hoverEl === el && el.dispatchEvent(new CustomEvent("drophover")), HOVER_MS);
}

/** Everything that depends on what is under the pointer. */
function at(x: number, y: number): HTMLElement | null {
  pointer = { x, y };
  const hit = document.elementFromPoint(x, y) as HTMLElement | null;
  const dest = hit?.closest<HTMLElement>("[data-drop]") ?? null;
  light(dest);
  report(dest);
  hover(hit?.closest<HTMLElement>("[data-drop-hover]") ?? null);
  return hit;
}

/** Near a scroll container's edge, keep scrolling until the pointer moves away. */
function autoscroll(hit: HTMLElement | null) {
  const box = hit?.closest<HTMLElement>("[data-drop-scroll]");
  edge = null;
  if (box) {
    const r = box.getBoundingClientRect();
    const top = pointer.y - r.top;
    const bottom = r.bottom - pointer.y;
    if (top < EDGE) edge = { box, step: -Math.ceil((EDGE - top) / 2) };
    else if (bottom < EDGE) edge = { box, step: Math.ceil((EDGE - bottom) / 2) };
  }
  if (edge && !edgeTimer) {
    edgeTimer = window.setInterval(() => {
      if (!edge) {
        clearInterval(edgeTimer);
        edgeTimer = 0;
        return;
      }
      edge.box.scrollTop += edge.step;
      // Rows moved under a still pointer: the destination may have changed.
      at(pointer.x, pointer.y);
    }, 30);
  }
}

function end() {
  light(null);
  hover(null);
  edge = null;
  sent = "";
}

/** Take over drag and drop for the window. Called once per page load. */
export function initDrag() {
  invoke("drop_install").catch(() => {});
  listen<Msg>("drag", ({ payload: m }) => {
    if (m.kind === "enter") {
      end();
    } else if (m.kind === "over") {
      const s = window.devicePixelRatio || 1;
      autoscroll(at(m.x / s, m.y / s));
    } else if (m.kind === "leave") {
      end();
    } else {
      end();
      dropOn(m.paths, m.dest, m.op);
    }
  });
}

/** `use:dropHover={fn}`: run `fn` when a drag rests on the element (switch tab, expand). */
export function dropHover(node: HTMLElement, fn: () => void) {
  node.dataset.dropHover = "";
  let run = fn;
  const on = () => run();
  node.addEventListener("drophover", on);
  return {
    update(next: () => void) {
      run = next;
    },
    destroy() {
      node.removeEventListener("drophover", on);
    },
  };
}
