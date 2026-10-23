// Archives as places: browsing a ZIP, 7z or tar like a folder, Extract all and
// its conflicts, hostile and damaged archives, opening an archive inside one,
// Compress to ZIP file, and cancelling with Windows' progress dialog.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// The small archives the Rust tests use (made by 7-Zip, PowerShell, and by
// hand for the hostile cases).
const fixtures = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../src-tauri/tests/archives");
const put = (dir, name, as = name) => {
  const p = path.join(dir, as);
  fs.copyFileSync(path.join(fixtures, name), p);
  return p;
};
const TREE = ["emoji 😀.txt", "empty", "plain.txt", "sub dir", "école.txt", "日本語.txt"].sort();
const sorted = async (t) => (await t.names()).sort();

/** A file's text, without the BOM PowerShell wrote into the fixtures. */
const text = (t, p) => t.read(p).replace(/^﻿/, "").trim();

/** Open Extract all for `zip`, set where it goes, and press Extract. */
async function extractAll(t, zip, dest, show = false) {
  await t.call("extractAll", zip);
  await t.waitFor("!!document.querySelector('.extractdlg')", 3000, "the Extract all dialog");
  await t.ev(`(() => { __e2e.extractDlg.dest = ${JSON.stringify(dest)}; __e2e.extractDlg.show = ${show}; })()`);
  await t.click(".extractdlg .btn.primary");
}

/** The newest activity line once it differs from `before`. */
async function nextNote(t, before, timeout = 20000) {
  await t.waitFor(`__e2e.activity.list[0] && __e2e.activity.list[0].id !== ${before}`, timeout, "the operation's report");
  return t.ev("__e2e.activity.list[0].title");
}
const lastId = (t) => t.ev("__e2e.activity.list[0]?.id ?? 0");

export default [
  {
    name: "a zip opens like a folder: nested folders, Unicode names, breadcrumbs, Up and typed paths",
    async run(t) {
      const d = t.dir("ar-browse");
      const zip = put(d, "7zip.zip", "tree.zip");
      const stamp = fs.statSync(zip).mtimeMs;
      await t.nav(zip + "|");
      t.eq(await sorted(t), TREE, "the archive's root");
      await t.nav(zip + "|sub dir\\deeper");
      t.eq(await t.names(), ["ünïcödé.md"], "two folders down");
      t.eq(await t.ev(`[...document.querySelectorAll(".crumb")].slice(-3).map((c) => c.textContent.trim())`), ["tree.zip", "sub dir", "deeper"], "breadcrumbs");
      await t.call("up");
      await t.settle();
      t.eq(await t.ev("__e2e.app.path"), zip + "|sub dir", "Up stays inside the archive");
      await t.nav(zip + "|");
      await t.call("up");
      await t.settle();
      t.eq(await t.ev("__e2e.app.path"), d, "Up from the archive's root is its folder");
      // A path typed through the archive goes inside it.
      const r = await t.ev(`__e2e.invoke("resolve_path", { input: ${JSON.stringify(zip + "\\sub dir")} })`);
      t.eq(r, { kind: "dir", path: zip + "|sub dir" }, "typed path");
      // The address bar shows the readable form.
      await t.nav(zip + "|sub dir");
      await t.click(".crumbfill");
      await t.waitFor("document.activeElement?.tagName === 'INPUT' && !!document.activeElement.value", 3000, "the address being edited");
      t.eq(await t.ev("document.activeElement.value"), zip + "\\sub dir", "address text");
      await t.key("Escape");
      // Nothing in it changes: Delete and F2 do nothing, and the zip is untouched.
      await t.nav(zip + "|");
      await t.select(["plain.txt"]);
      await t.key("Delete");
      await t.key("F2");
      await t.sleep(300);
      t.eq(await t.ev("__e2e.app.renaming"), -1, "no rename");
      t.eq(fs.statSync(zip).mtimeMs, stamp, "the archive is unchanged");
      t.eq(await sorted(t), TREE, "nothing deleted");
    },
  },
  {
    name: "search inside an archive finds names and kinds in every folder of it",
    async run(t) {
      const d = t.dir("ar-search");
      const zip = put(d, "7zip.zip", "tree.zip");
      await t.nav(zip + "|");
      await t.call("startSearch", "ünï", true);
      await t.waitFor("__e2e.app.search?.done", 10000, "search");
      await t.settle();
      t.eq(await t.names(), ["ünïcödé.md"], "a name two folders down");
      const folder = await t.ev(`[...document.querySelectorAll(".vrow .cell")].map((c) => c.textContent).find((x) => x.includes("tree.zip"))`);
      t.eq(folder, zip + "\\sub dir\\deeper", "the Folder column reads as a path");
      await t.call("startSearch", "kind:text", true);
      await t.waitFor("__e2e.app.search?.done", 10000, "search");
      await t.settle();
      t.eq(await sorted(t), ["emoji 😀.txt", "plain.txt", "école.txt", "ünïcödé.md", "日本語.txt"].sort(), "a kind");
      t.ok((await t.ev("document.querySelector('.statusbar').textContent")).includes("isn't searched"), "says what it didn't search");
    },
  },
  {
    name: "7z and tar.gz archives list the same tree as the zip",
    async run(t) {
      const d = t.dir("ar-formats");
      for (const f of ["sample.7z", "sample.tar.gz"]) {
        const a = put(d, f);
        await t.nav(a + "|");
        t.eq(await sorted(t), TREE, f);
        await t.nav(a + "|sub dir\\deeper");
        t.eq(await t.names(), ["ünïcödé.md"], `${f} two folders down`);
      }
    },
  },
  {
    name: "Extract all makes a folder named after the archive, shows it, and undo removes it",
    async run(t) {
      const d = t.dir("ar-extract");
      const zip = put(d, "7zip.zip", `tree-${t.token}.zip`);
      await t.nav(d);
      await t.select([`tree-${t.token}.zip`]);
      // The command bar offers it for a selected archive, as Explorer's does.
      await t.click(".cbtn", { where: { has: "Extract all" } });
      await t.waitFor("!!document.querySelector('.extractdlg')", 3000, "the Extract all dialog");
      const dest = path.join(d, `tree-${t.token}`);
      t.eq(await t.ev("__e2e.extractDlg.dest"), dest, "suggested destination");
      t.eq(await t.ev("__e2e.extractDlg.show"), true, "Show extracted files is on");
      const before = await lastId(t);
      await t.click(".extractdlg .btn.primary");
      t.eq(await nextNote(t, before), `Extracted tree-${t.token}.zip to tree-${t.token}.`, "report");
      await t.waitFor(`__e2e.app.path === ${JSON.stringify(dest)}`, 10000, "the extracted folder opens");
      await t.settle();
      t.eq(await sorted(t), TREE, "everything came out");
      t.eq(text(t, path.join(dest, "sub dir", "deeper", "ünïcödé.md")), "deep", "nested file");
      t.ok(fs.statSync(path.join(dest, "empty")).isDirectory(), "the empty folder too");
      await t.nav(d);
      await t.call("undo");
      await t.until(() => !fs.existsSync(dest), 10000, "undo recycled the folder");
    },
  },
  {
    name: "extracting into a folder with the same names asks Windows' question, and Skip keeps the old file",
    async run(t) {
      const d = t.dir("ar-conflict");
      const zip = put(d, "7zip.zip", "tree.zip");
      const dest = t.dir("ar-conflict/out");
      t.mk(path.join(dest, "plain.txt"), "mine");
      await t.nav(d);
      const before = await lastId(t);
      await extractAll(t, zip, dest);
      await t.uia("-Window", "Replace or Skip*", "-Click", "Skip this file");
      const note = await nextNote(t, before);
      t.eq(note, "Extracted tree.zip to out; 1 item kept as it was.", "report");
      t.eq(t.read(path.join(dest, "plain.txt")), "mine", "the old file stayed");
      t.eq(text(t, path.join(dest, "école.txt")), "accent", "the rest came out");
      t.eq(fs.readdirSync(dest).filter((n) => n.startsWith(".filezy-extract")), [], "no staging folder left");
    },
  },
  {
    name: "hostile names are never written outside the destination, and the report lists them",
    async run(t) {
      const d = t.dir("ar-hostile");
      const zip = put(d, "hostile.zip");
      const dest = path.join(d, "out");
      await t.nav(d);
      const before = await lastId(t);
      await extractAll(t, zip, dest);
      const note = await nextNote(t, before);
      t.ok(note.includes("6 items weren't extracted") && note.includes("3 names changed to work on Windows"), note);
      const problems = await t.ev("__e2e.activity.list[0].problems.map((p) => p.src).sort()");
      t.eq(problems, ["abs.txt", "dir\\escape.txt", "evil.txt", "link", "rooted.txt", "unc.txt"], "what wasn't extracted");
      const all = [];
      const walk = (p, rel = "") => {
        for (const n of fs.readdirSync(p)) {
          all.push(rel + n);
          if (fs.statSync(path.join(p, n)).isDirectory()) walk(path.join(p, n), rel + n + "\\");
        }
      };
      walk(dest);
      t.eq(all.sort(), ["_con.txt", "a_b.txt", "dir", "dir\\backslash.txt", "ok", "ok\\fine.txt", "trail__"], "only safe, valid names");
      t.eq(fs.readdirSync(d).sort(), ["hostile.zip", "out"], "nothing beside it");
      t.ok(!fs.existsSync(path.join(t.fixRoot, "evil.txt")) && !fs.existsSync(path.join(t.fixRoot, "escape.txt")), "nothing above it");
    },
  },
  {
    name: "a damaged archive says so, an empty one is an empty folder, a protected entry isn't extracted",
    async run(t) {
      const d = t.dir("ar-odd");
      const bad = put(d, "truncated.zip");
      t.eq(await t.call("navigate", bad + "|"), false, "can't open");
      t.ok((await t.ev("__e2e.app.status")).startsWith("This archive is damaged"), await t.ev("__e2e.app.status"));
      await t.nav(put(d, "empty.zip") + "|");
      t.eq(await t.ev("__e2e.app.total"), 0, "no items");
      t.ok((await t.ev("document.querySelector('.pane')?.textContent ?? ''")).includes("This folder is empty."), "says it's empty");
      const locked = put(d, "zipcrypto.zip");
      await t.nav(locked + "|");
      t.eq(await t.names(), ["plain.txt"], "a protected entry is listed");
      const before = await lastId(t);
      await extractAll(t, locked, path.join(d, "locked"));
      const note = await nextNote(t, before);
      t.eq(note, "Couldn't extract from zipcrypto.zip. Nothing in it can be extracted.", "report");
      t.ok(!fs.existsSync(path.join(d, "locked")), "no empty folder made");
      t.ok((await t.ev("__e2e.activity.list[0].problems[0]?.error ?? ''")).includes("password"), "says why");
    },
  },
  {
    name: "an archive inside an archive opens as a folder from a read-only copy",
    async run(t) {
      const d = t.dir("ar-nested");
      const inner = t.dir("ar-nested/src");
      put(inner, "7zip.zip", "inner.zip");
      const outer = path.join(d, "outer.zip");
      await t.psCmd(`Compress-Archive -Path '${path.join(inner, "inner.zip")}' -DestinationPath '${outer}'`);
      await t.nav(outer + "|");
      t.eq(await t.names(), ["inner.zip"], "outer");
      await t.call("openEntry", 0);
      await t.waitFor("__e2e.app.path.endsWith('inner.zip|')", 10000, "inside the inner archive");
      await t.settle();
      t.eq(await sorted(t), TREE, "the inner archive's files");
      const copy = (await t.ev("__e2e.app.path")).slice(0, -1);
      t.ok(copy.toLowerCase().includes("\\filezy\\open\\"), copy);
      t.ok(!(fs.statSync(copy).mode & 0o200), "the copy is read-only");
    },
  },
  {
    name: "Compress to ZIP file makes a zip beside the items, ready to rename, and its contents read back",
    async run(t) {
      const d = t.dir("ar-compress");
      t.mk(path.join(d, `notes-${t.token}`, "inside.txt"), "in");
      t.mk(path.join(d, "loose.txt"), "loose");
      await t.nav(d);
      await t.select([`notes-${t.token}`, "loose.txt"]);
      // The zip is named after the item the menu was opened on.
      await t.ev(`__e2e.app.focus = __e2e.rows.findIndex((r) => r?.name === ${JSON.stringify(`notes-${t.token}`)})`);
      await t.ev("__e2e.hooks.contextMenu()");
      await t.waitFor("!!document.querySelector('.ctxmenu')", 3000, "the item menu");
      await t.click(".ctxmenu .menu-item", { where: { has: "Compress to ZIP file" } });
      const zip = path.join(d, `notes-${t.token}.zip`);
      await t.until(() => fs.existsSync(zip), 15000, "the zip appears");
      await t.waitFor("__e2e.app.renaming >= 0", 5000, "the new zip is ready to rename");
      t.eq(await t.ev("__e2e.listing().then((r) => r[__e2e.app.renaming]?.name)"), `notes-${t.token}.zip`, "renaming the zip");
      await t.key("Escape");
      t.eq(await t.zipEntries(zip), ["loose.txt", `notes-${t.token}/`, `notes-${t.token}/inside.txt`], "its contents");
      t.eq(fs.readdirSync(d).filter((n) => n.includes("filezy-part")), [], "no part file left");
      await t.call("undo");
      await t.until(() => !fs.existsSync(zip), 10000, "undo recycled the zip");
    },
  },
  {
    name: "Cancel in Windows' progress dialog stops an extraction and leaves nothing behind",
    async run(t) {
      const d = t.dir("ar-cancel");
      const src = t.dir("ar-cancel/src");
      const zip = path.join(d, "big.zip");
      // 48 MB that doesn't compress, stored: with the runner's pause per 256 KB,
      // extracting it takes about five seconds.
      await t.psCmd(
        `$b = New-Object byte[] (48MB); (New-Object Random 1).NextBytes($b); [IO.File]::WriteAllBytes('${path.join(src, "big.bin")}', $b);` +
          `Add-Type -AssemblyName System.IO.Compression.FileSystem; [IO.Compression.ZipFile]::CreateFromDirectory('${src}', '${zip}', [IO.Compression.CompressionLevel]::NoCompression, $false)`,
      );
      await t.nav(d);
      const dest = path.join(d, "big");
      const before = await lastId(t);
      await extractAll(t, zip, dest);
      await t.uia("-Window", "Extracting big.zip", "-Click", "Cancel");
      t.eq(await nextNote(t, before, 30000), "Cancelled. Nothing was extracted.", "report");
      t.ok(!fs.existsSync(dest), "no folder left");
    },
  },
  {
    name: "the item menu offers Extract all for an archive, and only opening and extracting inside one",
    async run(t) {
      const d = t.dir("ar-menus");
      const zip = put(d, "7zip.zip", "tree.zip");
      await t.nav(d);
      await t.select(["tree.zip"]);
      await t.ev("__e2e.hooks.contextMenu()");
      await t.waitFor("!!document.querySelector('.ctxmenu') && !document.querySelector('.ctxmenu .menuload')", 60000, "the item menu");
      // Each entry starts with its icon glyph.
      const labels = () => t.ev(`[...document.querySelectorAll(".ctxmenu > .menu-item")].map((b) => b.textContent.replace(/^[\uE000-\uF8FF]/, "").trim())`);
      t.ok((await labels()).includes("Extract all..."), "Extract all for a zip");
      await t.key("Escape");
      await t.nav(zip + "|");
      await t.select(["plain.txt"]);
      await t.ev("__e2e.hooks.contextMenu()");
      await t.waitFor("!!document.querySelector('.ctxmenu')", 3000, "the item menu inside");
      t.eq(await labels(), ["Open", "Extract...", "Extract all..."], "inside the archive");
      t.eq(await t.ev("document.querySelectorAll('.ctxmenu .strip').length"), 0, "no Cut/Copy/Rename/Delete strip");
      await t.key("Escape");
    },
  },
];
