// Checksums, Compare and Folder sizes (tools.rs, tools.svelte.ts, ToolDialog).
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

export default [
  {
    name: "checksums of a known file match node's, and a pasted checksum is matched",
    async run(t) {
      const d = t.dir("tools-hash");
      const body = "Filezy checksum test\n";
      t.mk(path.join(d, "known.txt"), body);
      await t.nav(d);
      await t.select(["known.txt"]);
      await t.call("checksumsSel");
      await t.waitFor("__e2e.tool.kind === 'hash' && !__e2e.tool.busy && __e2e.tool.hashes.length === 1", 10000, "hashes computed");
      const sha = crypto.createHash("sha256").update(body).digest("hex");
      t.eq(await t.ev("__e2e.tool.hashes[0].hashes.find(([a]) => a === 'SHA-256')?.[1]"), sha, "SHA-256");
      await t.ev(`(() => { const i = document.querySelector("dialog .field input"); i.value = ${JSON.stringify(sha.toUpperCase())}; i.dispatchEvent(new Event("input", { bubbles: true })); })()`);
      await t.waitFor("document.querySelector('dialog .verdict.ok')?.textContent.includes('known.txt')", 3000, "the pasted checksum matches");
      await t.call("closeTool");
      t.eq(await t.ev("!!document.querySelector('dialog')"), false, "the dialog closed");
    },
  },
  {
    name: "folder sizes on 3,000 items stays responsive, lists the largest first and shows 200 until asked",
    async run(t) {
      const d = t.dir("tools-sizes");
      for (let i = 0; i < 3000; i++) fs.writeFileSync(path.join(d, `f${String(i).padStart(4, "0")}.txt`), "x".repeat(i % 7));
      t.mk(path.join(d, "big.bin"), "y".repeat(50_000));
      await t.nav(d);
      // A timer that keeps firing proves the page never stopped answering.
      await t.ev("window.__gap = 0; window.__last = performance.now(); window.__tick = setInterval(() => { const n = performance.now(); window.__gap = Math.max(window.__gap, n - window.__last); window.__last = n; }, 20)");
      await t.call("analyzeFolder");
      await t.waitFor("__e2e.tool.kind === 'sizes' && !__e2e.tool.busy && __e2e.sizes.list.length === 3001", 30000, "every item measured");
      const gap = await t.ev("(clearInterval(window.__tick), window.__gap)");
      t.ok(gap < 1000, `the page answered throughout (longest gap ${Math.round(gap)} ms)`);
      t.eq(await t.ev("__e2e.sizes.list[0].path.endsWith('big.bin')"), true, "largest first");
      t.eq(await t.ev("document.querySelectorAll('dialog .srow').length"), 200, "200 rows shown");
      await t.ev("document.querySelector('dialog .more').click()");
      await t.waitFor("document.querySelectorAll('dialog .srow').length === 3001", 5000, "Show all lists every item");
      await t.call("closeTool");
    },
  },
  {
    name: "Stop during a folder sizes scan ends it and keeps what was measured",
    async run(t) {
      const d = t.dir("tools-stop");
      for (let i = 0; i < 40; i++) {
        const sub = path.join(d, `sub${i}`);
        fs.mkdirSync(sub);
        for (let j = 0; j < 300; j++) fs.writeFileSync(path.join(sub, `${j}.txt`), "z");
      }
      await t.nav(d);
      await t.call("analyzeFolder");
      await t.waitFor("__e2e.tool.busy", 5000, "measuring");
      await t.call("stopTool");
      await t.waitFor("!__e2e.tool.busy", 5000, "stopped");
      const n = await t.ev("__e2e.sizes.list.length");
      t.ok(n <= 40, `kept ${n} measured items`);
      await t.call("closeTool");
    },
  },
];
