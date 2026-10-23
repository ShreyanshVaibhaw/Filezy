// The shell's context-menu entries as the backend reads them (shellmenu.rs).
import { invoke } from "@tauri-apps/api/core";
import { extraOn, rememberExtras } from "./settings.svelte";

export interface ShellItem {
  id: number;
  text: string;
  verb: string | null;
  sep: boolean;
  disabled: boolean;
  sub: ShellItem[] | null;
  /** A choice in Filezy's own menus built from these: shown with a check mark. */
  checked?: boolean;
}

/** One built menu; `session` goes back with every command run from it. */
export interface ShellMenu {
  session: number;
  items: ShellItem[];
  /** Why there are no shell entries, when there aren't any. */
  note: string | null;
}

/// Shell verbs whose entries Filezy draws itself, so the shell's copies are
/// dropped from the extension list. Windows' own menu hands these back to the
/// page instead of running them (the `handled` list of shellmenu.rs native()).
export const OUR_VERBS = new Set([
  "open", "opennewwindow", "opennewtab", "opennewprocess", "cut", "copy", "paste", "delete", "rename",
  "properties", "openas", "pintohome", "unpinfromhome", "pintohomefile", "copyaspath", "link", "windows.modernshare",
  "makeavailableoffline", "makeavailableonline",
]);

/// Keep a shown menu current. Some extensions add entries after the menu was
/// built (Send to fills in its targets from a background thread for items in
/// several folders), which a real menu shows as they arrive. Reads `menu`
/// again while `showing()` holds, for a few seconds, and hands `update` each
/// version that changed.
export function followMenu(menu: ShellMenu, showing: () => boolean, update: (items: ShellItem[]) => void) {
  let last = JSON.stringify(menu.items);
  let reads = 0;
  const read = () => {
    if (!showing() || ++reads > 10) return;
    invoke<ShellItem[]>("shell_menu_items", { session: menu.session })
      .then((items) => {
        if (!showing()) return;
        const now = JSON.stringify(items);
        if (now !== last) {
          last = now;
          update(items);
        }
        setTimeout(read, 300);
      })
      .catch(() => {}); // a newer menu replaced it
  };
  setTimeout(read, 300);
}

/// The extension entries to show after the native ones: no duplicates of
/// those (`native` verbs), none the user switched off in Settings, and no
/// separators doubled or left at either end.
export function filterExtras(items: ShellItem[], native: Set<string>): ShellItem[] {
  const out: ShellItem[] = [];
  for (const it of items) {
    if (it.sep) {
      if (out.length && !out[out.length - 1].sep) out.push(it);
      continue;
    }
    if (native.has((it.verb ?? "").toLowerCase())) continue;
    out.push(it);
  }
  while (out.length && out[out.length - 1].sep) out.pop();
  // Everything the shell offered is switchable in Settings, whether or not
  // it is currently switched on.
  rememberExtras(out.filter((it) => !it.sep).map((it) => it.text));
  return out.filter((it) => it.sep || extraOn(it.text));
}
