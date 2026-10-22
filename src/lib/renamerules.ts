// Bulk rename rules (F02's numbered rename plus O01's find/replace and
// templates). Pure, so the node tests check them; BatchRename.svelte previews
// the names and ops.svelte.ts renames through rename_many.

export type RenameMode = "number" | "replace" | "template";
export interface RenameRule {
  mode: RenameMode;
  /** number: "Base (1)", "Base (2)"... */
  base: string;
  /** replace: in each name without its extension. */
  find: string;
  replace: string;
  regex: boolean;
  matchCase: boolean;
  /** template: {name} {n} {n:3} {date} {parent}; the extension is kept. */
  template: string;
  start: number;
}
export interface RenameItem {
  path: string;
  is_dir: boolean;
  /** Modified time in ms, for {date}. */
  modified?: number;
}

const BAD = /[\\/:*?"<>|]/;

function split(it: RenameItem): [string, string] {
  const p = it.path.replace(/[\\/]+$/, "");
  const n = p.slice(p.lastIndexOf("\\") + 1);
  const dot = it.is_dir ? -1 : n.lastIndexOf(".");
  return dot > 0 ? [n.slice(0, dot), n.slice(dot)] : [n, ""];
}

function parentName(p: string): string {
  const parts = p.split("\\").filter(Boolean);
  return parts.length > 1 ? parts[parts.length - 2].replace(/:$/, "") : "";
}

function ymd(ms: number | undefined): string {
  if (!ms) return "";
  const d = new Date(ms);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/** New names in list order, or the first reason the rule can't be applied. */
export function ruleNames(rule: RenameRule, items: RenameItem[]): { names: string[]; error: string } {
  let re: RegExp | null = null;
  if (rule.mode === "replace" && rule.find) {
    try {
      const src = rule.regex ? rule.find : rule.find.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      re = new RegExp(src, rule.matchCase ? "g" : "gi");
    } catch (e) {
      return { names: items.map((it) => split(it).join("")), error: `The pattern isn't valid: ${(e as Error).message}` };
    }
  }
  const start = Number.isFinite(rule.start) ? Math.trunc(rule.start) : 1;
  const names = items.map((it, i) => {
    const [stem, ext] = split(it);
    const n = start + i;
    switch (rule.mode) {
      case "number":
        return `${rule.base.trim()} (${n})${ext}`;
      case "replace":
        return (re ? stem.replace(re, rule.replace) : stem) + ext;
      case "template":
        return (
          rule.template.replace(/\{(name|n(?::(\d{1,2}))?|date|parent)\}/g, (_, key: string, pad?: string) =>
            key === "name" ? stem : key === "date" ? ymd(it.modified) : key === "parent" ? parentName(it.path) : String(n).padStart(Number(pad ?? 0), "0"),
          ).trim() + ext
        );
    }
  });
  const bad = names.find((nm) => !nm.replace(/\.[^.]*$/, "").trim() || BAD.test(nm) || /[. ]$/.test(nm));
  const error =
    rule.mode === "replace" && !rule.find
      ? "Type the text to find."
      : bad === undefined
        ? // Search results can come from several folders: only a clash within one counts.
          new Set(names.map((nm, i) => `${items[i].path.slice(0, items[i].path.lastIndexOf("\\"))}\\${nm}`.toLowerCase())).size < names.length
          ? "Two items would get the same name."
          : ""
        : BAD.test(bad)
          ? 'A file name can\'t contain any of the following characters: \\ / : * ? " < > |'
          : `"${bad}" isn't a usable name.`;
  return { names, error };
}
