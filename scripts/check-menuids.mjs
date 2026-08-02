// Fails if the context-menu registry and the menus that read it drift apart:
// an id used in a component but missing from menuitems.ts would be a menu entry
// with no switch in Settings; a registry id no component reads would be a switch
// that does nothing. Run with `npm run check:menuids`.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const LIB = "src/lib";
const registry = readFileSync(join(LIB, "menuitems.ts"), "utf8");
const declared = new Set([...registry.matchAll(/id:\s*"([^"]+)"/g)].map((m) => m[1]));

const used = new Map(); // id -> file
for (const f of readdirSync(LIB).filter((f) => f.endsWith(".svelte"))) {
  const src = readFileSync(join(LIB, f), "utf8");
  for (const m of src.matchAll(/\b(?:menuOn|setMenuOn)\("([^"]+)"/g)) used.set(m[1], f);
  // Ids listed in the anyMenuOn() guard arrays count as used too.
  for (const m of src.matchAll(/"((?:file|blank|drive|nav|tab)\.[a-z]+)"/g)) {
    if (!used.has(m[1])) used.set(m[1], f);
  }
}

const problems = [];
for (const [id, f] of used) if (!declared.has(id)) problems.push(`${f}: "${id}" is not in menuitems.ts`);
for (const id of declared) if (!used.has(id)) problems.push(`menuitems.ts: "${id}" is never used by a menu`);

if (problems.length) {
  console.error("menu id drift:\n  " + problems.join("\n  "));
  process.exit(1);
}
console.log(`menu ids ok: ${declared.size} declared, all used`);
