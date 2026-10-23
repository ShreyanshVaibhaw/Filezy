import { execFileSync } from "node:child_process";
import path from "node:path";

const icacls = (...a) => execFileSync("icacls.exe", a, { windowsHide: true });
const me = process.env.USERNAME;

export default [
  {
    name: "the page runs only the app's own code: inline scripts, eval and outside requests are refused",
    async run(t) {
      // The policy refuses before anything is sent, so example.com is never contacted.
      const r = await t.ev(`new Promise((done) => {
        const seen = [];
        const note = (e) => seen.push(e.violatedDirective);
        document.addEventListener("securitypolicyviolation", note);
        window.__inline = false;
        const s = document.createElement("script");
        s.textContent = "window.__inline = true";
        document.head.appendChild(s);
        s.remove();
        new Image().src = "https://example.com/x.png";
        fetch("https://example.com/").catch(() => {});
        // From a task of the page's own: code DevTools evaluates is exempt from the policy.
        setTimeout(() => {
          let evals = true;
          try { eval("1"); } catch { evals = false; }
          setTimeout(() => {
            document.removeEventListener("securitypolicyviolation", note);
            done({ inline: window.__inline, evals, seen });
          }, 300);
        });
      })`);
      t.eq(r.inline, false, "an inline script didn't run");
      t.eq(r.evals, false, "eval threw");
      t.ok(["script-src-elem", "img-src", "connect-src"].every((d) => r.seen.includes(d)), `reported: ${r.seen}`);
    },
  },
  {
    name: "corrupt saved settings and folder views don't stop the app",
    async run(t) {
      const d = t.dir("corrupt-prefs");
      t.mk(path.join(d, "a.txt"));
      // Some listing requests first: the reloaded page starts its own numbering
      // again and must still outrank these.
      for (const sub of ["one", "two", "three"]) await t.nav(t.dir(path.join("corrupt-prefs-before", sub)));
      await t.ev(`(() => {
        localStorage.setItem("settings", "{not json");
        localStorage.setItem("folderPrefs", JSON.stringify({ "c:\\\\x": { s: 5, v: "nope" }, "c:\\\\ok": { s: "size", a: false, v: "list", g: "none" } }));
        globalThis.__beforeReload = true;
        location.reload();
      })()`).catch(() => {});
      await t.waitFor("!globalThis.__beforeReload && !!globalThis.__e2e && __e2e.app.path !== ''", 30000, "app came back after reload");
      t.eq(await t.ev("__e2e.settings.theme"), "system", "settings fell back to defaults");
      await t.nav(d);
      t.eq(await t.names(), ["a.txt"], "folders still open");
      const prefs = await t.ev(`JSON.parse(localStorage.getItem("folderPrefs") ?? "{}")`);
      t.eq(Object.keys(prefs), ["c:\\ok"], "the malformed folder view was dropped, the valid one kept");
    },
  },
  {
    name: "a start folder that no longer exists opens Home and says why",
    async run(t) {
      await t.call("setSetting", "startPage", "C:\\no\\such\\start-folder");
      try {
        await t.call("newTab");
        await t.settle();
        t.eq(await t.ev("__e2e.app.path"), "::Home", "fell back to Home");
        t.ok((await t.ev("__e2e.app.status")).includes("start folder"), await t.ev("__e2e.app.status"));
      } finally {
        await t.call("setSetting", "startPage", "::Home");
      }
    },
  },
  {
    name: "a folder you can't open says so and the app stays where it was",
    async run(t) {
      const ok = t.dir("denied-open/ok");
      const locked = t.dir("denied-open/locked");
      t.mk(path.join(ok, "here.txt"));
      t.mk(path.join(locked, "secret.txt"));
      icacls(locked, "/deny", `${me}:(RD)`);
      try {
        await t.nav(ok);
        t.eq(await t.call("navigate", locked), false, "navigation refused");
        t.eq(await t.ev("__e2e.app.path"), ok, "still in the previous folder");
        t.ok((await t.ev("__e2e.app.status")).includes("permission"), await t.ev("__e2e.app.status"));
        t.eq(await t.names(), ["here.txt"], "previous listing intact");
      } finally {
        icacls(locked, "/remove:d", me);
      }
    },
  },
  {
    name: "a folder that becomes unreadable shows why in place, and Try again recovers",
    async run(t) {
      const d = t.dir("denied-later");
      t.mk(path.join(d, "f.txt"));
      await t.nav(d);
      icacls(d, "/deny", `${me}:(RD)`);
      try {
        await t.call("refresh");
        await t.settle();
        t.ok((await t.ev("__e2e.app.error")).includes("permission"), "error shown in place");
        t.eq(await t.ev("__e2e.app.total"), 0, "no stale rows left to act on");
        t.ok(await t.ev("!!document.querySelector('.state.error')"), "error view rendered");
      } finally {
        icacls(d, "/remove:d", me);
      }
      await t.click(".sbtn", { where: { text: "Try again" } });
      await t.waitFor("__e2e.app.error === '' && __e2e.app.total === 1", 5000, "recovered");
    },
  },
  {
    name: "a failed Recycle Bin restore explains itself",
    async run(t) {
      const d = t.dir("bin-fail");
      const name = `vanishing-${t.token}.txt`;
      const f = t.mk(path.join(d, name), "v");
      await t.nav(d);
      await t.select([name]);
      await t.call("del");
      await t.until(() => !t.exists(f), 5000, "recycled");
      await t.nav("::RecycleBin");
      await t.click(await t.binRow({ has: name }));
      // Someone else restores it before we do.
      await t.ps("recycle-restore.ps1", "-Match", t.token);
      await t.until(() => t.exists(f), 10000, "restored elsewhere");
      await t.click(".abtn", { where: { has: "Restore selected" } });
      await t.waitFor("__e2e.app.status.includes('no longer in the Recycle Bin')", 10000, "message shown");
      t.ok(await t.ev("!!document.querySelector('.notice')"), "visible on the bin page");
    },
  },
];
