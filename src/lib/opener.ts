import { settings } from "./settings.svelte";

/// Event handlers for a tile that opens something, as file rows do: on
/// double-click, on Enter or Space (a click with no pointer, detail 0), or on a
/// single click when "Single-click to open items" is on.
export function opener(open: () => void) {
  return {
    onclick: (e: MouseEvent) => {
      if (e.detail === 0 || settings.singleClick) open();
    },
    ondblclick: () => {
      if (!settings.singleClick) open();
    },
  };
}
