// The search box's text is the query in Windows' Advanced Query Syntax
// ("report kind:document datemodified:this week"), so filters picked from the
// Search options menu read back as plain text and can be edited by hand.

/** Each filter's key, and the other names Windows accepts for it. */
const ALIASES: Record<string, string[]> = {
  kind: ["kind"],
  ext: ["ext", "type", "fileext"],
  size: ["size"],
  datemodified: ["datemodified", "modified", "date"],
  datecreated: ["datecreated", "created"],
};

/** Values Explorer writes with a space ("datemodified:this week"). */
const TWO_WORDS = ["this week", "last week", "this month", "last month", "this year", "last year"];

/// The query's tokens: "quoted phrases" whole, and two-word date values kept with their key.
export function tokens(q: string): string[] {
  const raw: string[] = [];
  let cur = "";
  let quoted = false;
  for (const c of q.trim()) {
    if (c === '"') quoted = !quoted;
    if (/\s/.test(c) && !quoted) {
      if (cur) raw.push(cur);
      cur = "";
    } else cur += c;
  }
  if (cur) raw.push(cur);
  const out: string[] = [];
  for (let i = 0; i < raw.length; i++) {
    const t = raw[i];
    const val = t.slice(t.indexOf(":") + 1).toLowerCase();
    if (t.includes(":") && i + 1 < raw.length && TWO_WORDS.includes(`${val} ${raw[i + 1].toLowerCase()}`)) {
      out.push(`${t} ${raw[++i]}`);
    } else out.push(t);
  }
  return out;
}

/** The filter key a token sets (by any of its names), or null for a word. */
function keyOf(token: string): string | null {
  const m = /^([a-z]+):/i.exec(token);
  if (!m) return null;
  const k = m[1].toLowerCase();
  return Object.keys(ALIASES).find((key) => ALIASES[key].includes(k)) ?? null;
}

/// The query with filter `key` set to `value` (replacing any value it had,
/// under any of its names), or removed when `value` is null.
export function withFilter(q: string, key: string, value: string | null): string {
  const kept = tokens(q).filter((t) => keyOf(t) !== key);
  if (value !== null) kept.push(`${key}:${value}`);
  return kept.join(" ");
}

/// The value a filter has in the query, or null.
export function filterOf(q: string, key: string): string | null {
  const t = tokens(q).find((x) => keyOf(x) === key);
  return t ? t.slice(t.indexOf(":") + 1).replace(/^=/, "").toLowerCase() : null;
}

/// Just the words: every filter taken out.
export function withoutFilters(q: string): string {
  return tokens(q)
    .filter((t) => keyOf(t) === null)
    .join(" ");
}
