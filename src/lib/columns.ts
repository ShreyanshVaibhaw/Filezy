// The Details view's columns: what can be shown, and each folder's chosen
// set, order and widths (Explorer remembers them per folder). Name is always
// first and not listed here.

export type ColKey =
  | "modified"
  | "type"
  | "size"
  | "created"
  | "location"
  | "attributes"
  | "status"
  | "title"
  | "author"
  | "tags"
  | "category"
  | "dimensions"
  | "duration";

export interface ColDef {
  key: ColKey;
  label: string;
  width: number;
  /** Right-aligned, like Explorer's Size. */
  right?: boolean;
  /** Read from the file's property handler, a few rows at a time. */
  prop?: boolean;
}

export const COLUMNS: ColDef[] = [
  { key: "modified", label: "Date modified", width: 170 },
  { key: "type", label: "Type", width: 150 },
  { key: "size", label: "Size", width: 110, right: true },
  { key: "created", label: "Date created", width: 170 },
  { key: "location", label: "Folder", width: 240 },
  { key: "attributes", label: "Attributes", width: 90 },
  { key: "status", label: "Status", width: 240 },
  { key: "title", label: "Title", width: 180, prop: true },
  { key: "author", label: "Authors", width: 150, prop: true },
  { key: "tags", label: "Tags", width: 150, prop: true },
  { key: "category", label: "Categories", width: 150, prop: true },
  { key: "dimensions", label: "Dimensions", width: 110, prop: true },
  { key: "duration", label: "Length", width: 90, prop: true },
];

export const colDef = (k: ColKey) => COLUMNS.find((c) => c.key === k)!;

export interface Col {
  k: ColKey;
  w: number;
}

const DEFAULT: ColKey[] = ["modified", "type", "size"];
/** Search results also say where each item is, as in Explorer. */
const SEARCH: ColKey[] = ["location", ...DEFAULT];
const STORE = "folderColumns";
/** Search results keep one layout of their own. */
export const SEARCH_KEY = "::search";

function valid(v: unknown): v is Col[] {
  return (
    Array.isArray(v) &&
    v.length <= COLUMNS.length &&
    v.every((c) => c && COLUMNS.some((d) => d.key === c.k) && typeof c.w === "number" && c.w >= 40 && c.w <= 2000) &&
    new Set(v.map((c) => c.k)).size === v.length
  );
}

/// Saved layouts, with anything malformed dropped (and the cleaned set written back).
function load(): Record<string, Col[]> {
  const out: Record<string, Col[]> = {};
  try {
    const raw = JSON.parse(localStorage.getItem(STORE) ?? "{}");
    const entries = raw && typeof raw === "object" && !Array.isArray(raw) ? Object.entries(raw) : [];
    for (const [k, v] of entries) if (valid(v)) out[k] = v;
    if (entries.length !== Object.keys(out).length) localStorage.setItem(STORE, JSON.stringify(out));
  } catch {
    localStorage.removeItem(STORE);
  }
  return out;
}

let saved: Record<string, Col[]> = load();

const keyOf = (path: string) => path.replace(/\\+$/, "").toLowerCase();

/// The columns a folder (or the search results, SEARCH_KEY) opens with. A
/// folder a cloud provider syncs also shows Status first, as in Explorer.
export function columnsFor(path: string, cloud = false): Col[] {
  const own = saved[keyOf(path)];
  if (own) return own.map((c) => ({ ...c }));
  const keys = path === SEARCH_KEY ? SEARCH : cloud ? (["status", ...DEFAULT] as ColKey[]) : DEFAULT;
  return keys.map((k) => ({ k, w: colDef(k).width }));
}

export function saveColumns(path: string, cols: Col[]) {
  if (!valid(cols)) return;
  saved[keyOf(path)] = cols.map((c) => ({ k: c.k, w: Math.round(c.w) }));
  const keys = Object.keys(saved);
  // ponytail: drop the oldest insertion when the map grows unbounded
  if (keys.length > 500) delete saved[keys[0]];
  localStorage.setItem(STORE, JSON.stringify(saved));
}

/// Explorer's "Reset folders" forgets the columns too.
export function clearColumns() {
  saved = {};
  localStorage.removeItem(STORE);
}

/// Explorer's Attributes column: R read-only, H hidden, S system, A archive,
/// C compressed, E encrypted, O offline, P pinned, U unpinned, M recall on access.
export function attrLetters(a: number): string {
  const letters: [number, string][] = [
    [0x1, "R"], [0x2, "H"], [0x4, "S"], [0x20, "A"], [0x800, "C"],
    [0x4000, "E"], [0x1000, "O"], [0x80000, "P"], [0x100000, "U"], [0x400000, "M"],
  ];
  return letters.filter(([bit]) => a & bit).map(([, c]) => c).join("");
}
