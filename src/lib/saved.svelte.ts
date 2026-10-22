// Saved searches and workspaces (O01): named local configurations the user
// makes, renames and deletes. Kept apart from settings because they're lists
// of records, validated here on load so a damaged blob drops only bad entries.
import { app, tabs, navigate, newTab, startSearch, setSearchScope } from "./store.svelte";
import { prompt } from "./ask.svelte";
import { parseSaved, type SavedSearch, type Workspace } from "./savedparse";
export type { SavedSearch, Workspace };

const KEY = "saved";

export const saved = $state(parseSaved(localStorage.getItem(KEY)));

function store() {
  localStorage.setItem(KEY, JSON.stringify(saved));
}

/** Add, replacing an entry of the same name (names are what the menus show). */
function put<T extends { name: string }>(list: T[], item: T) {
  const i = list.findIndex((x) => x.name.toLowerCase() === item.name.toLowerCase());
  if (i >= 0) list[i] = item;
  else list.push(item);
  store();
}

export async function saveSearch() {
  const query = app.search?.query ?? app.filter.trim();
  const root = app.search?.root ?? app.path;
  if (!query || !root) return;
  const name = await prompt("Save search", "Name this search. It will be listed under Search options and on the Settings page.", query);
  if (name) put(saved.searches, { name, root, query, scope: app.searchScope });
}

export async function runSearch(s: SavedSearch) {
  if (!(await navigate(s.root))) return;
  setSearchScope(s.scope);
  app.filter = s.query;
  await startSearch(s.query, true);
}

export async function saveWorkspace() {
  const paths = tabs.map((t) => t.path).filter(Boolean);
  if (!paths.length) return;
  const name = await prompt("Save workspace", paths.length === 1 ? "Name this tab. Opening the workspace later adds it as a tab." : `Name this set of ${paths.length} tabs. Opening it later adds them as tabs.`);
  if (name) put(saved.workspaces, { name, tabs: paths });
}

export async function openWorkspace(w: Workspace) {
  for (const p of w.tabs) await newTab(p);
}

export async function renameSaved(kind: "searches" | "workspaces", i: number) {
  const item = saved[kind][i];
  if (!item) return;
  const name = await prompt("Rename", "New name:", item.name, "Rename");
  if (name) {
    item.name = name;
    store();
  }
}

export function deleteSaved(kind: "searches" | "workspaces", i: number) {
  saved[kind].splice(i, 1);
  store();
}
