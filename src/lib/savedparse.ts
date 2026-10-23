// Validation for saved.svelte.ts, apart so the node tests can load it.
export type SearchScope = "folder" | "subfolders" | "drive" | "all";

export interface SavedSearch {
  name: string;
  root: string;
  query: string;
  scope: SearchScope;
}
export interface Workspace {
  name: string;
  tabs: string[];
}

const SCOPES = ["folder", "subfolders", "drive", "all"];
const str = (v: unknown): v is string => typeof v === "string" && v.length > 0;

export function parseSaved(json: string | null): { searches: SavedSearch[]; workspaces: Workspace[] } {
  let raw: { searches?: unknown; workspaces?: unknown } = {};
  try {
    raw = JSON.parse(json ?? "{}") ?? {};
  } catch {
    // Damaged: start empty rather than not start.
  }
  const list = (v: unknown) => (Array.isArray(v) ? v : []);
  // Names are what the menus show and key their lists by: the first of a name wins.
  const unique = <T extends { name: string }>(items: T[]) => {
    const seen = new Set<string>();
    return items.filter((x) => !seen.has(x.name.toLowerCase()) && !!seen.add(x.name.toLowerCase()));
  };
  return {
    searches: unique(list(raw.searches).filter(
      (s): s is SavedSearch => !!s && str(s.name) && str(s.root) && str(s.query) && SCOPES.includes(s.scope),
    )),
    workspaces: unique(list(raw.workspaces).filter(
      (w): w is Workspace => !!w && str(w.name) && Array.isArray(w.tabs) && w.tabs.length > 0 && w.tabs.every(str),
    )),
  };
}
