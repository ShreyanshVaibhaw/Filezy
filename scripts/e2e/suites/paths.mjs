import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const attrib = (p, ...flags) => execFileSync("attrib.exe", [...flags, p], { windowsHide: true });

export default [
  {
    name: "folders past 260 characters list, rename and copy their paths",
    async run(t) {
      let d = t.dir("long");
      while (d.length < 280) d = path.join(d, "abcdefghijklmnopqrstuvwxyz0123456789");
      fs.mkdirSync(d, { recursive: true });
      t.mk(path.join(d, "deep.txt"), "x");
      await t.nav(d);
      t.eq(await t.ev("__e2e.app.path"), d, "address");
      t.eq(await t.names(), ["deep.txt"], "listing");
      await t.select(["deep.txt"]);
      t.eq(await t.copyAsPath(), `"${path.join(d, "deep.txt")}"`, "copy as path");
      await t.call("commitRename", await t.ev("[...__e2e.sel][0]"), "deeper.txt");
      await t.settle();
      t.ok(t.exists(path.join(d, "deeper.txt")), "renamed on disk");
      t.eq(await t.selectedNames(), ["deeper.txt"], "renamed item selected");
    },
  },
  {
    name: "names with spaces, accents, CJK and emoji round-trip",
    async run(t) {
      const d = t.dir("unicode names");
      const names = ["naïve résumé.txt", "日本語のファイル.txt", "party 🎉.txt"];
      for (const n of names) t.mk(path.join(d, n), n);
      t.dir(path.join("unicode names", "Ünïcödé dir"));
      await t.nav(d);
      t.eq((await t.names()).sort(), ["Ünïcödé dir", ...names].sort(), "listing");
      await t.select(["party 🎉.txt"]);
      await t.call("commitRename", await t.ev("[...__e2e.sel][0]"), "fiesta 🎊 ñ.txt");
      await t.settle();
      t.eq(t.read(path.join(d, "fiesta 🎊 ñ.txt")), "party 🎉.txt", "renamed file content");
      await t.nav(path.join(d, "Ünïcödé dir"));
      t.eq(await t.ev("__e2e.app.path"), path.join(d, "Ünïcödé dir"), "unicode folder opens");
    },
  },
  {
    name: "a case-only rename is allowed and invalid names are explained",
    async run(t) {
      const d = t.dir("rename-rules");
      t.mk(path.join(d, "readme.txt"));
      t.mk(path.join(d, "other.txt"));
      await t.nav(d);
      await t.select(["readme.txt"]);
      await t.call("commitRename", await t.ev("[...__e2e.sel][0]"), "README.txt");
      await t.settle();
      t.ok(fs.readdirSync(d).includes("README.txt"), "case changed on disk");
      await t.select(["README.txt"]);
      await t.call("commitRename", await t.ev("[...__e2e.sel][0]"), "a:b.txt");
      t.ok((await t.ev("__e2e.app.status")).includes("can't contain"), "invalid character explained");
      await t.call("commitRename", await t.ev("[...__e2e.sel][0]"), "con.txt");
      t.ok((await t.ev("__e2e.app.status")).includes("device name"), "reserved name explained");
      await t.call("commitRename", await t.ev("[...__e2e.sel][0]"), "other.txt");
      t.ok((await t.ev("__e2e.app.status")).includes("already"), "collision explained");
      t.ok(t.exists(path.join(d, "other.txt")) && t.exists(path.join(d, "README.txt")), "nothing overwritten");
    },
  },
  {
    name: "search lists a junction to an ancestor once and doesn't loop through it",
    async run(t) {
      const d = t.dir("junction-loop");
      t.mk(path.join(d, "sub", "needle.txt"));
      fs.symlinkSync(d, path.join(d, "sub", "loop-needle"), "junction");
      await t.nav(d);
      await t.call("startSearch", "needle");
      await t.waitFor("__e2e.app.search?.done", 15000, "search should finish");
      await t.settle();
      t.eq((await t.names()).sort(), ["loop-needle", "needle.txt"], "results");
    },
  },
  {
    name: "a junction opens under its own path, not its target's",
    async run(t) {
      const d = t.dir("junction-open");
      const target = t.dir(path.join("junction-open", "target"));
      t.mk(path.join(target, "inside.txt"));
      const link = path.join(d, "link");
      fs.symlinkSync(target, link, "junction");
      await t.nav(link);
      t.eq(await t.ev("__e2e.app.path"), link, "address keeps the link");
      t.eq(await t.names(), ["inside.txt"], "contents");
    },
  },
  {
    name: "addresses with quotes, variables, wrong case and .. resolve like Explorer",
    async run(t) {
      const d = t.dir("Addr Test");
      t.dir(path.join("Addr Test", "SubFolder"));
      t.mk(path.join(d, "SubFolder", "File.TXT"));
      const r = (input) => t.ev(`__e2e.invoke("resolve_path", { input: ${JSON.stringify(input)} })`);
      t.eq(await r(`"${d}\\subfolder"`), { kind: "dir", path: path.join(d, "SubFolder") }, "quoted, wrong case");
      t.eq(await r(`${d}\\SubFolder\\..\\subfolder\\file.txt`), { kind: "file", path: path.join(d, "SubFolder", "File.TXT") }, "dotdot and file");
      // Not %TEMP%: the runner gives the app a %TEMP% of its own.
      const viaVar = d.replace(process.env.USERPROFILE ?? "", "%USERPROFILE%");
      if (viaVar !== d) t.eq(await r(viaVar), { kind: "dir", path: d }, "environment variable");
      const missing = await t.ev(`__e2e.invoke("resolve_path", { input: "C:\\\\no\\\\such\\\\folder" }).then(() => "", (e) => String(e))`);
      t.ok(missing.startsWith("Windows can't find"), missing);
    },
  },
  {
    name: "hidden and protected system items follow Explorer's two settings",
    async run(t) {
      const d = t.dir("visibility");
      const plain = t.mk(path.join(d, "plain.txt"));
      const hidden = t.mk(path.join(d, "hidden.txt"));
      const prot = t.mk(path.join(d, "protected.txt"));
      const sys = t.mk(path.join(d, "systemonly.txt"));
      attrib(hidden, "+h");
      attrib(prot, "+h", "+s");
      attrib(sys, "+s");
      const show = async (h, p) => {
        await t.call("setSetting", "showHidden", h);
        await t.call("setSetting", "showProtected", p);
        await t.nav(d);
        return (await t.names()).sort();
      };
      try {
        t.eq(await show(false, false), ["plain.txt", "systemonly.txt"], "defaults");
        t.eq(await show(true, false), ["hidden.txt", "plain.txt", "systemonly.txt"], "hidden items on");
        t.eq(await show(true, true), ["hidden.txt", "plain.txt", "protected.txt", "systemonly.txt"], "protected on");
      } finally {
        await t.call("setSetting", "showHidden", false);
        await t.call("setSetting", "showProtected", false);
        attrib(prot, "-h", "-s");
        attrib(hidden, "-h");
        attrib(sys, "-s");
        void plain;
      }
    },
  },
  {
    name: "UNC paths browse, go up, and copy as UNC paths",
    async run(t) {
      const d = t.dir("unc-root");
      t.dir(path.join("unc-root", "child"));
      t.mk(path.join(d, "child", "u.txt"));
      const u = t.unc(path.join(d, "child"));
      await t.nav(u);
      t.eq((await t.ev("__e2e.app.path")).toLowerCase(), u.toLowerCase(), "address stays UNC");
      t.eq(await t.names(), ["u.txt"], "listing");
      await t.select(["u.txt"]);
      t.eq((await t.copyAsPath()).toLowerCase(), `"${u}\\u.txt"`.toLowerCase(), "copy as path");
      await t.call("up");
      await t.settle();
      t.eq((await t.ev("__e2e.app.path")).toLowerCase(), t.unc(d).toLowerCase(), "up stays on the share");
      t.eq(await t.selectedNames(), ["child"], "came-from folder selected");
    },
  },
];
