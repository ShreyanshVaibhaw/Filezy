import { pathKey } from "./paths";

/// A record per folder, kept in localStorage under `key`. Entries `valid`
/// rejects are dropped on load and the cleaned map is written back, so a
/// damaged blob can't stop startup or linger.
export function perFolderMap<T>(key: string, valid: (v: unknown) => v is T) {
  let map: Record<string, T> = {};
  try {
    const raw = JSON.parse(localStorage.getItem(key) ?? "{}");
    const entries = raw && typeof raw === "object" && !Array.isArray(raw) ? Object.entries(raw) : [];
    for (const [k, v] of entries) if (valid(v)) map[k] = v;
    if (entries.length !== Object.keys(map).length) localStorage.setItem(key, JSON.stringify(map));
  } catch {
    localStorage.removeItem(key);
  }
  return {
    get: (path: string): T | undefined => map[pathKey(path)],
    set(path: string, value: T) {
      map[pathKey(path)] = value;
      const keys = Object.keys(map);
      // ponytail: drop the oldest insertion past 500; an LRU if folders churn
      if (keys.length > 500) delete map[keys[0]];
      localStorage.setItem(key, JSON.stringify(map));
    },
    clear() {
      map = {};
      localStorage.removeItem(key);
    },
    size: () => Object.keys(map).length,
  };
}
