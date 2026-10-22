// Display-path helpers mirroring src-tauri/src/paths.rs. Pure functions with no
// imports, so `node --test` can run their tests directly.

/** "::Home", "::RecycleBin" and other pages that are not folders. */
export function isVirtual(p: string): boolean {
  return p.startsWith("::");
}

/**
 * A folder inside an archive: the archive's path, "|", and the folder in it
 * ("C:\a.zip|docs\2024"; "C:\a.zip|" is its root). "|" can't occur in a
 * Windows path, so one is never mistaken for a file system path.
 */
export function isArchive(p: string): boolean {
  return p.includes("|");
}

export function splitArchive(p: string): { file: string; inner: string } | null {
  const i = p.indexOf("|");
  return i < 0 ? null : { file: p.slice(0, i), inner: p.slice(i + 1).replace(/^\\+|\\+$/g, "") };
}

/** The path as people read and type it: "C:\a.zip\docs" for "C:\a.zip|docs". */
export function displayPath(p: string): string {
  const a = splitArchive(p);
  return a ? (a.inner ? `${a.file}\\${a.inner}` : a.file) : p;
}

/** Archives Filezy can open, by name. */
export function isArchiveName(name: string): boolean {
  return /\.(zip|7z|tar|tgz|tar\.gz)$/i.test(name);
}

export function isUnc(p: string): boolean {
  return /^\\\\[^\\?.][^\\]*/.test(p);
}

/** WSL's server and distro names ignore case; Linux path components do not. */
export function isWsl(p: string): boolean {
  return /^\\\\wsl(?:\.localhost|\$)\\[^\\]+/i.test(p);
}

export function pathKey(p: string): string {
  const trimmed = p.replace(/\\+$/, "");
  if (!isWsl(trimmed)) return trimmed.toLowerCase();
  const parts = trimmed.slice(2).split("\\");
  return "\\\\" + parts.slice(0, 2).join("\\").toLowerCase() + (parts.length > 2 ? "\\" + parts.slice(2).join("\\") : "");
}

/** "\\server" alone: a computer, whose items are its shares. */
export function isServer(p: string): boolean {
  return /^\\\\[^\\?.][^\\]*\\?$/.test(p);
}

/** Root of a path: "C:\" or "\\server\share\"; null for a bare server or virtual page. */
export function rootOf(p: string): string | null {
  const drive = /^([a-zA-Z]:)(\\|$)/.exec(p);
  if (drive) return drive[1] + "\\";
  const unc = /^\\\\([^\\]+)\\([^\\]+)/.exec(p);
  return unc ? `\\\\${unc[1]}\\${unc[2]}\\` : null;
}

/** Containing folder; the parent of "\\srv\share" is "\\srv"; null at a top. */
export function parentPath(p: string): string | null {
  if (isVirtual(p)) return null;
  const a = splitArchive(p);
  if (a) {
    // An archive's root goes up to the folder holding the archive.
    if (!a.inner) return parentPath(a.file);
    const i = a.inner.lastIndexOf("\\");
    return `${a.file}|${i < 0 ? "" : a.inner.slice(0, i)}`;
  }
  const t = p.replace(/\\+$/, "");
  if (/^[a-zA-Z]:$/.test(t)) return null;
  if (isUnc(t)) {
    const parts = t.slice(2).split("\\");
    if (isWsl(t) && parts.length === 2) return null;
    if (parts.length <= 1) return null;
    return "\\\\" + parts.slice(0, -1).join("\\");
  }
  const i = t.lastIndexOf("\\");
  if (i < 0) return null;
  return i <= 2 ? t.slice(0, 2) + "\\" : t.slice(0, i);
}

export function joinPath(dir: string, name: string): string {
  if (dir.endsWith("|")) return dir + name;
  return dir.replace(/\\+$/, "") + "\\" + name;
}

/** A library: a .library-ms file, shown as the folders it names. */
export function isLibrary(p: string): boolean {
  return /\.library-ms$/i.test(p);
}

/** Last component: "Users" for "C:\Users"; a root keeps its own form ("C:\"); a library is named without its extension. */
export function baseName(p: string): string {
  const a = splitArchive(p);
  if (a) return a.inner ? a.inner.slice(a.inner.lastIndexOf("\\") + 1) : baseName(a.file);
  if (isLibrary(p)) return baseName(p.slice(0, -".library-ms".length));
  const t = p.replace(/\\+$/, "");
  if (/^[a-zA-Z]:$/.test(t)) return t + "\\";
  const i = t.lastIndexOf("\\");
  return i < 0 || i === t.length - 1 ? p : t.slice(i + 1);
}

/** Breadcrumb segments: "C:\a\b" -> C:\, a, b; "\\srv\sh\a" -> \\srv, sh, a. */
export function crumbs(p: string): { name: string; path: string }[] {
  // A library sits under the folder holding it, as in Explorer's "Libraries > Documents".
  if (isLibrary(p)) {
    const dir = parentPath(p);
    return [...(dir ? [{ name: baseName(dir), path: dir }] : []), { name: baseName(p), path: p }];
  }
  const a = splitArchive(p);
  if (a) {
    // The archive is one crumb, its folders the next ones.
    const out = crumbs(a.file);
    out[out.length - 1] = { ...out[out.length - 1], path: `${a.file}|` };
    let acc = "";
    for (const part of a.inner ? a.inner.split("\\") : []) {
      acc = acc ? `${acc}\\${part}` : part;
      out.push({ name: part, path: `${a.file}|${acc}` });
    }
    return out;
  }
  const t = p.replace(/\\+$/, "");
  const out: { name: string; path: string }[] = [];
  if (isUnc(t)) {
    const parts = t.slice(2).split("\\");
    if (isWsl(t)) {
      let location = "\\\\" + parts[0] + "\\" + parts[1];
      out.push({ name: parts[1], path: location });
      for (const part of parts.slice(2)) { location += "\\" + part; out.push({ name: part, path: location }); }
      return out;
    }
    let acc = "\\\\" + parts[0];
    out.push({ name: parts[0], path: acc });
    for (const part of parts.slice(1)) {
      acc += "\\" + part;
      out.push({ name: part, path: acc });
    }
    return out;
  }
  const parts = t.split("\\");
  let acc = "";
  for (const part of parts) {
    acc = acc ? acc + "\\" + part : part;
    out.push({ name: part, path: acc.endsWith(":") ? acc + "\\" : acc });
  }
  return out;
}

/** Case-insensitive path equality, ignoring trailing backslashes. */
export function samePath(a: string, b: string): boolean {
  return pathKey(a) === pathKey(b);
}

/** True when `p` is `dir` itself or inside it. */
export function isWithin(p: string, dir: string): boolean {
  const a = pathKey(p);
  const b = pathKey(dir);
  // An archive's root ("C:\a.zip|") holds everything after its "|".
  return a === b || a.startsWith(b.endsWith("|") ? b : b + "\\");
}
