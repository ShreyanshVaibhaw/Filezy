import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

/// Copy or cut the selection and make sure it is really on the clipboard: other
/// programs on a test machine can replace the clipboard at any moment.
async function toClipboard(t, cut) {
  const want = (await t.selectedNames()).length;
  for (let i = 0; i < 4; i++) {
    await t.call("copySel", cut);
    const c = await t.clipboard();
    if (c.files.length === want && c.effect === (cut ? 2 : 5)) return;
    await t.sleep(200);
  }
  throw new Error("another program kept replacing the clipboard");
}

// Cut A, B, C into a folder that already has B, then answer the conflict dialog.
async function cutIntoConflict(t, name) {
  const s = t.dir(`${name}/S`);
  const d = t.dir(`${name}/D`);
  for (const n of ["c1.txt", "c2.txt", "c3.txt"]) t.mk(path.join(s, n), `src ${n}`);
  t.mk(path.join(d, "c2.txt"), "existing");
  await t.nav(s);
  await t.call("selectAll");
  await toClipboard(t, true);
  await t.nav(d);
  await t.ev("void __e2e.paste()");
  return { s, d };
}

/// Check the cut clipboard kept only the unmoved item. Another program may
/// write the clipboard mid-operation; the app then rightly leaves it alone, and
/// says so in the result, so that case is reported instead of asserted.
async function expectCutLeft(t, file) {
  const how = await t.ev("__e2e.activity.list[0]?.result.clipboard");
  if (how === "changed elsewhere") {
    console.log("        (clipboard changed by another program during the paste; left alone as designed)");
    return;
  }
  t.eq(how, "rewritten", "clipboard decision");
  const clip = await t.clipboard();
  t.eq(clip.files, [file], "clipboard files");
  t.eq(clip.effect, 2, "clipboard still holds a cut");
}

const moved = (t, s, d) => () =>
  !t.exists(path.join(s, "c1.txt")) && !t.exists(path.join(s, "c3.txt")) && t.exists(path.join(d, "c3.txt"));

/** Wait for the operation the page started to settle (no undo in flight, listing idle). */
const settleOp = async (t) => {
  await t.sleep(300);
  await t.settle();
};

export default [
  {
    name: "a cut-paste cancelled at a conflict keeps the unmoved item cut",
    async run(t) {
      const { s, d } = await cutIntoConflict(t, "cut-cancel");
      await t.uia("-Window", "Replace or Skip*", "-Close");
      await t.until(moved(t, s, d), 10000, "c1 and c3 should have moved");
      await settleOp(t);
      t.eq(t.read(path.join(d, "c2.txt")), "existing", "destination c2");
      t.ok(t.exists(path.join(s, "c2.txt")), "c2 should stay in the source");
      await expectCutLeft(t, path.join(s, "c2.txt"));
      t.ok((await t.ev("__e2e.app.status")).startsWith("Cancelled after moved 2 of 3"), await t.ev("__e2e.app.status"));
    },
  },
  {
    name: "a skipped conflict stays cut and undo leaves the existing file alone",
    async run(t) {
      const { s, d } = await cutIntoConflict(t, "cut-skip");
      await t.uia("-Window", "Replace or Skip*", "-Click", "Skip this file");
      await t.until(moved(t, s, d), 10000, "c1 and c3 should have moved");
      await settleOp(t);
      await expectCutLeft(t, path.join(s, "c2.txt"));
      t.ok((await t.ev("__e2e.app.status")).startsWith("Moved 2 of 3 items, 1 skipped"), await t.ev("__e2e.app.status"));

      await t.call("undo");
      await t.until(() => t.exists(path.join(s, "c1.txt")) && t.exists(path.join(s, "c3.txt")), 10000, "undo should move c1 and c3 back");
      await settleOp(t);
      t.eq(t.read(path.join(d, "c2.txt")), "existing", "the destination's own c2 must stay");
      t.eq(t.read(path.join(s, "c2.txt")), "src c2.txt", "the skipped source c2 must stay");
    },
  },
  {
    name: "another app's copy during a cut-paste wins: the unmoved item is not put back on the clipboard",
    async run(t) {
      const { s, d } = await cutIntoConflict(t, "cut-replaced");
      // While Windows asks about c2, another app copies something else.
      await t.ps("clip.ps1", "-Text", "copied elsewhere meanwhile");
      await t.uia("-Window", "Replace or Skip*", "-Click", "Skip this file");
      await t.until(moved(t, s, d), 10000, "c1 and c3 should have moved");
      await settleOp(t);
      t.eq(await t.ev("__e2e.activity.list[0]?.result.clipboard"), "changed elsewhere", "the paste noticed the new copy");
      t.ok(!(await t.clipboard()).files.includes(path.join(s, "c2.txt")), "c2 was not put back as a cut");
    },
  },
  {
    name: "undo won't remove a file that replaced an existing one",
    async run(t) {
      const s = t.dir("replace-undo/S");
      const d = t.dir("replace-undo/D");
      t.mk(path.join(s, "doc.txt"), "new contents");
      t.mk(path.join(d, "doc.txt"), "old contents");
      await t.nav(s);
      await t.select(["doc.txt"]);
      await toClipboard(t, false);
      await t.nav(d);
      await t.ev("void __e2e.paste()");
      await t.uia("-Window", "Replace or Skip*", "-Click", "Replace the file in the destination");
      await t.until(() => t.read(path.join(d, "doc.txt")) === "new contents", 10000, "replace should happen");
      await settleOp(t);
      await t.call("undo");
      await settleOp(t);
      t.ok(t.exists(path.join(d, "doc.txt")), "undo must not delete the file that replaced another");
      t.ok((await t.ev("__e2e.app.status")).includes("can't be brought back"), await t.ev("__e2e.app.status"));
    },
  },
  {
    name: "undo asks before removing a copy that was edited afterwards",
    async run(t) {
      const s = t.dir("edited-undo/S");
      const d = t.dir("edited-undo/D");
      const name = `notes-${t.token}.txt`;
      t.mk(path.join(s, name), "v1");
      await t.nav(s);
      await t.select([name]);
      await toClipboard(t, false);
      await t.nav(d);
      await t.call("paste");
      await settleOp(t);
      const copy = path.join(d, name);
      t.ok(t.exists(copy), "copied");
      fs.writeFileSync(copy, "v2, edited after the copy");
      await t.ev("void __e2e.undo()");
      const msg = await t.answer(false);
      t.ok(msg.includes(name), `dialog names the file: ${msg}`);
      await settleOp(t);
      t.ok(t.exists(copy), "declining keeps the edited copy");
      await t.ev("void __e2e.undo()");
      await t.answer(true);
      await t.until(() => !t.exists(copy), 10000, "accepting recycles it");
      t.ok(t.exists(path.join(s, name)), "the source is untouched");
    },
  },
  {
    name: "undo and redo of a move, and of a rename",
    async run(t) {
      const s = t.dir("redo/S");
      const d = t.dir("redo/D");
      t.mk(path.join(s, "m.txt"), "m");
      await t.nav(s);
      await t.select(["m.txt"]);
      await toClipboard(t, true);
      await t.nav(d);
      await t.call("paste");
      await settleOp(t);
      t.ok(t.exists(path.join(d, "m.txt")), "moved");
      await t.call("undo");
      await settleOp(t);
      t.ok(t.exists(path.join(s, "m.txt")) && !t.exists(path.join(d, "m.txt")), "undo moved it back");
      await t.call("redo");
      await settleOp(t);
      t.ok(t.exists(path.join(d, "m.txt")) && !t.exists(path.join(s, "m.txt")), "redo moved it again");

      await t.nav(d);
      await t.select(["m.txt"]);
      await t.call("commitRename", await t.ev("[...__e2e.sel][0]"), "renamed.txt");
      await settleOp(t);
      t.ok(t.exists(path.join(d, "renamed.txt")), "renamed");
      await t.key("z", { ctrl: true });
      await settleOp(t);
      t.ok(t.exists(path.join(d, "m.txt")) && !t.exists(path.join(d, "renamed.txt")), "Ctrl+Z restored the name");
      await t.key("y", { ctrl: true });
      await settleOp(t);
      t.ok(t.exists(path.join(d, "renamed.txt")), "Ctrl+Y renamed it again");
    },
  },
  {
    name: "undo puts back a move between a share and a local folder",
    async run(t) {
      const s = t.dir("xvol/S");
      const d = t.dir("xvol/D");
      t.mk(path.join(s, "x.txt"), "across");
      // The same folder through the loopback admin share is another volume to the shell.
      const us = "\\\\localhost\\" + s[0].toUpperCase() + "$" + s.slice(2);
      await t.nav(us);
      await t.select(["x.txt"]);
      await toClipboard(t, true);
      await t.nav(d);
      await t.call("paste");
      await settleOp(t);
      t.ok(t.exists(path.join(d, "x.txt")) && !t.exists(path.join(s, "x.txt")), "moved off the share");
      await t.call("undo");
      await settleOp(t);
      t.eq(t.read(path.join(s, "x.txt")), "across", "undo moved it back to the share");
      t.ok(!t.exists(path.join(d, "x.txt")), "and out of the local folder");
    },
  },
  {
    name: "Shift+Delete deletes for good only after Windows asks",
    async run(t) {
      const d = t.dir("perm-delete");
      const name = `gone-${t.token}.txt`;
      const f = t.mk(path.join(d, name), "x");
      await t.nav(d);
      await t.select([name]);
      const undoBefore = await t.ev("__e2e.history.undo.length");
      await t.key("Delete", { shift: true });
      await t.uia("-Window", "*Delete*", "-Command", "2"); // No
      await settleOp(t);
      t.ok(t.exists(f), "No keeps the file");
      t.eq(await t.ev("__e2e.app.status"), "", "declining says nothing");
      await t.key("Delete", { shift: true });
      await t.uia("-Window", "*Delete*", "-Command", "6"); // Yes
      await t.until(() => !t.exists(f), 10000, "Yes deletes it");
      await settleOp(t);
      const inBin = await t.ev(`__e2e.invoke("recycle_list").then((l) => l.items.some((i) => i.name === ${JSON.stringify(name)}))`);
      t.ok(!inBin, "a permanent delete must not land in the Recycle Bin");
      t.eq(await t.ev("__e2e.history.undo.length"), undoBefore, "no undo entry for a permanent delete");
    },
  },
  {
    name: "a copy into a folder you can't write to is reported, then retried",
    async run(t) {
      const s = t.dir("denied/S");
      const d = t.dir("denied/D");
      t.mk(path.join(s, "a.txt"), "a");
      execFileSync("icacls.exe", [d, "/deny", `${process.env.USERNAME}:(W)`], { windowsHide: true });
      try {
        await t.nav(s);
        await t.select(["a.txt"]);
        await toClipboard(t, false);
        await t.ev(`void __e2e.invoke("paste", { dest: ${JSON.stringify(d)} }).then((r) => (window.__denied = r))`);
        await t.uia("-Window", "*Access Denied*", "-Click", "Cancel");
        const r = await t.waitFor("window.__denied", 10000, "paste result");
        t.eq(r.items.map((i) => i.state), ["unattempted"], "item outcome");
        t.ok(r.cancelled, "reported as cancelled");
        t.ok(!t.exists(path.join(d, "a.txt")), "nothing written");
      } finally {
        execFileSync("icacls.exe", [d, "/remove:d", process.env.USERNAME], { windowsHide: true });
      }
    },
  },
  {
    name: "moving a file that is in use reports it and Retry finishes the job",
    async run(t) {
      const s = t.dir("in-use/S");
      const d = t.dir("in-use/D");
      const f = t.mk(path.join(s, "locked.txt"), "l");
      const holder = spawn("powershell.exe", ["-NoProfile", "-Command", `$h=[System.IO.File]::Open('${f}','Open','ReadWrite','None'); Start-Sleep 60`], { stdio: "ignore", windowsHide: true });
      try {
        await t.sleep(1500);
        await t.nav(s);
        await t.select(["locked.txt"]);
        await toClipboard(t, true);
        await t.nav(d);
        await t.ev("void __e2e.paste()");
        await t.uia("-Window", "*complete*", "-Click", "Cancel");
        await settleOp(t);
        t.eq(await t.ev("__e2e.app.status"), "Cancelled. Nothing was moved.", "message");
        t.ok(await t.ev("!!__e2e.activity.current?.retry"), "Retry offered");
        t.ok(t.exists(f), "still in the source");
      } finally {
        holder.kill();
      }
      await t.sleep(500);
      await t.click(".notice .act", { where: { text: "Retry" } });
      await t.until(() => t.exists(path.join(d, "locked.txt")), 10000, "Retry moved it");
    },
  },
];
