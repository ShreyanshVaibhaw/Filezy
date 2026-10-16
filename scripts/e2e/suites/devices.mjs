// Drives and the network: This PC following drives as Windows adds and
// removes them, drives that can't be read shown with their state, the
// Network page and a computer reached by name, and Windows' Map Network Drive.
//
// `subst` stands in for a USB drive: it makes a drive letter Windows
// announces like any other, for this user's session only. Each test removes
// the letters it makes.
import fs from "node:fs";
import path from "node:path";

/** A drive letter nothing uses now. */
function freeLetter(used = []) {
  for (const l of "QRSTUVWXYPONML") if (!used.includes(l) && !fs.existsSync(`${l}:\\`)) return l;
  throw new Error("no free drive letter");
}
const subst = (t, args) => t.psCmd(`subst ${args}`);
const drive = (t, letter) => t.ev(`JSON.stringify(__e2e.app.drives.find((d) => d.path === "${letter}:\\\\") ?? null)`).then(JSON.parse);

export default [
  {
    name: "a drive Windows adds or removes appears on This PC and in the navigation pane by itself",
    async run(t) {
      const target = t.dir("dev-subst");
      t.mk(path.join(target, "on-the-drive.txt"), "x");
      const l = freeLetter();
      await t.nav("::ThisPC");
      try {
        await subst(t, `${l}: "${target}"`);
        await t.waitFor(`__e2e.app.drives.some((d) => d.path === "${l}:\\\\")`, 10000, "the new drive on This PC");
        t.ok(await t.ev(`[...document.querySelectorAll(".drivetile")].some((b) => b.title === "${l}:\\\\")`), "a tile for it");
        t.ok(await t.ev(`[...document.querySelectorAll(".navitem, .treeitem, [role=treeitem]")].some((b) => b.textContent.includes("(${l}:)"))`), "in the navigation pane");
        await t.nav(`${l}:\\`);
        t.eq(await t.names(), ["on-the-drive.txt"], "it opens");
        await t.nav("::ThisPC");
      } finally {
        await subst(t, `${l}: /d`).catch(() => {});
      }
      await t.waitFor(`!__e2e.app.drives.some((d) => d.path === "${l}:\\\\")`, 10000, "the drive gone from This PC");
    },
  },
  {
    name: "a drive that can't be read is shown with Windows' reason instead of being left out",
    async run(t) {
      const target = t.dir("dev-gone");
      const l = freeLetter();
      try {
        await subst(t, `${l}: "${target}"`);
        // The folder behind the letter disappears: the drive is still there but can't be read.
        fs.rmSync(target, { recursive: true, force: true });
        await t.nav("::ThisPC");
        await t.call("loadDrives");
        const d = await drive(t, l);
        t.ok(d, "still listed");
        t.eq(d.state, "error", "its state");
        t.ok(d.note && d.note.length > 5, `Windows' reason: ${d.note}`);
        const text = await t.ev(`[...document.querySelectorAll(".drivetile")].find((b) => b.title === "${l}:\\\\")?.querySelector(".state")?.textContent ?? ""`);
        t.eq(text, d.note, "the tile says why");
      } finally {
        await subst(t, `${l}: /d`).catch(() => {});
      }
    },
  },
  {
    name: "the Network page reaches a computer by name and shows its shares under Network",
    async run(t) {
      await t.nav("::Network");
      await t.waitFor("!__e2e.network.loading", 15000, "network discovery");
      const found = await t.ev("__e2e.network.places.length");
      if (!found) t.ok((await t.ev("document.querySelector('.netpane .note')?.textContent ?? ''")).includes("No computers were found"), "says none were found");
      await t.click("#net-path");
      await t.key("a", { ctrl: true });
      await t.type("\\\\localhost");
      await t.key("Enter");
      await t.waitFor("__e2e.app.path.toLowerCase() === '\\\\\\\\localhost'", 10000, "this computer opens");
      await t.settle();
      const names = await t.names();
      t.ok(names.every((n) => !n.endsWith("$")), `no hidden shares: ${names}`);
      t.eq(await t.ev(`[...document.querySelectorAll(".crumb")].map((c) => c.textContent.trim()).slice(0, 2)`), ["Network", "localhost"], "breadcrumbs");
      t.ok(await t.ev(`document.querySelector('[data-key="::Network"]')?.classList.contains("active")`), "Network is current in the navigation pane");
      t.eq(await t.ev("__e2e.can('new') || __e2e.can('paste')"), false, "nothing is made in a computer's list of shares");
      if (!names.length) t.ok((await t.ev("document.querySelector('.state')?.textContent ?? ''")).includes("doesn't share any folders"), "says it shares none");
    },
  },
  {
    name: "Map network drive opens Windows' own dialog, and closing it changes nothing",
    async run(t) {
      await t.nav("::ThisPC");
      const before = await t.ev("__e2e.app.drives.length");
      await t.click(".cbtn", { where: { title: "See more" } });
      await t.click(".menu-item", { where: { has: "Map network drive" } });
      await t.uia("-Window", "Map Network Drive", "-Close");
      await t.sleep(500);
      t.eq(await t.ev("__e2e.app.drives.length"), before, "no drive added");
    },
  },
];
