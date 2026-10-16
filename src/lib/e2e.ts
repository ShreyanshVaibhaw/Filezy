// Test hook for the native E2E runner (scripts/e2e). Installed only when the
// app was started with FILEZY_E2E=1, so normal runs expose nothing extra.
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import * as store from "./store.svelte";
import * as ops from "./ops.svelte";
import * as selection from "./selection.svelte";
import * as archive from "./archive.svelte";
import { settings, setSetting, setTheme } from "./settings.svelte";
import { dialog } from "./ask.svelte";
import type { Entry } from "./store.svelte";

let fetching = 0;
/// Every row of the current listing. A watcher refresh can replace the listing
/// between reading the revision and the call; wait it out and read again.
async function listing(): Promise<Entry[]> {
  fetching++;
  try {
    for (let i = 0; ; i++) {
      while (!store.listingIdle()) await new Promise((r) => setTimeout(r, 20));
      try {
        return await invoke<Entry[]>("get_range", { rev: store.app.rev, start: 0, count: store.app.total });
      } catch (e) {
        if (i >= 10 || !String(e).startsWith("The folder changed")) throw e;
      }
    }
  } finally {
    fetching--;
  }
}

invoke<boolean>("e2e_enabled")
  .then((on) => {
    if (!on) return;
    (globalThis as Record<string, unknown>).__e2e = {
      ...store,
      ...ops,
      ...selection,
      ...archive,
      settings,
      setSetting,
      setTheme,
      dialog,
      invoke,
      idle: () => store.listingIdle() && !ops.opsBusy() && !fetching,
      // Close the window as a user would, so WebView2 saves what the page stored.
      closeWindow: () => getCurrentWindow().close(),
      // The window's size and state, for the window placement tests.
      win: getCurrentWindow(),
      LogicalSize,
      listing,
      async selectNames(names: string[]) {
        const all = await listing();
        store.clearSel();
        all.forEach((e, i) => names.includes(e.name) && store.sel.add(i));
        if (store.sel.size) store.app.anchor = Math.min(...store.sel);
      },
      async selectedNames() {
        const all = await listing();
        return all.filter((_, i) => store.isSelected(i)).map((e) => e.name);
      },
    };
  })
  .catch(() => {});
