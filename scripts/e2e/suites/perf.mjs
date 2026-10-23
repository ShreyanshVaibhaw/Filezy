import fs from "node:fs";
import path from "node:path";

// Opt-in (npm run test:e2e -- --perf): numbers for the record, not pass/fail
// gates. The listing is warm in the OS cache; a cold listing needs a reboot or
// an admin cache flush and is not measured. The 100,000-file folder stays in
// %TEMP%\filezy-perf-100k between runs; delete it to reclaim the space.

const N = 100_000;

async function workingSetMB(t) {
  // The app plus the WebView2 processes it started.
  const out = await t.psCmd(`
    $root = ${t.pid};
    $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, WorkingSetSize;
    $ids = @($root); $grew = $true;
    while ($grew) { $grew = $false; foreach ($p in $all) { if ($ids -contains $p.ParentProcessId -and -not ($ids -contains $p.ProcessId)) { $ids += $p.ProcessId; $grew = $true } } }
    $app = ($all | Where-Object { $_.ProcessId -eq $root }).WorkingSetSize;
    $sum = ($all | Where-Object { $ids -contains $_.ProcessId } | Measure-Object WorkingSetSize -Sum).Sum;
    "{0} {1} {2}" -f [math]::Round($app / 1MB), [math]::Round($sum / 1MB), $ids.Count`);
  const [app, total, procs] = out.split(" ").map(Number);
  return { app, total, procs };
}

export default [
  {
    name: "listing and scrolling 100,000 files",
    perf: true,
    async run(t) {
      // Kept between runs (outside the per-run folder): writing 100,000 files takes ~45 s.
      const d = path.join(path.dirname(t.root), "filezy-perf-100k");
      fs.mkdirSync(d, { recursive: true });
      if (fs.readdirSync(d).length !== N) {
        const t0 = Date.now();
        for (let i = 0; i < N; i++) fs.writeFileSync(path.join(d, `file-${String(i).padStart(6, "0")}.txt`), "");
        console.log(`        created ${N.toLocaleString()} files in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
      }
      await t.nav(path.dirname(d));

      const open = async () => {
        await t.nav(path.dirname(d));
        const ms = await t.ev(`(async () => {
          const t0 = performance.now();
          await __e2e.navigate(${JSON.stringify(d)});
          while (!__e2e.idle() || !__e2e.rowAt(0)) await new Promise((r) => setTimeout(r, 1));
          return performance.now() - t0;
        })()`);
        return Math.round(ms);
      };
      const runs = [];
      for (let i = 0; i < 5; i++) runs.push(await open());
      t.eq(await t.ev("__e2e.app.total"), N, "all files listed");
      console.log(`        open to first rows, warm: ${runs.join(", ")} ms (median ${[...runs].sort((a, b) => a - b)[2]} ms)`);

      // Scroll through the virtualized list one viewport per frame and time frames.
      const frames = await t.ev(`(async () => {
        const sc = document.querySelector(".scroller");
        const times = [];
        let last = performance.now();
        for (let i = 0; i < 300; i++) {
          sc.scrollTop += sc.clientHeight;
          await new Promise((r) => requestAnimationFrame(r));
          const now = performance.now();
          times.push(now - last);
          last = now;
        }
        return times;
      })()`);
      const sorted = [...frames].sort((a, b) => a - b);
      const p = (q) => sorted[Math.floor(q * (sorted.length - 1))].toFixed(1);
      const slow = frames.filter((f) => f > 34).length;
      console.log(`        scrolling a viewport per frame: median ${p(0.5)} ms, p95 ${p(0.95)} ms, max ${p(1)} ms, ${slow} of ${frames.length} frames over 34 ms`);

      const mem = await workingSetMB(t);
      console.log(`        working set with the 100k listing: app ${mem.app} MB, app + WebView2 ${mem.total} MB (${mem.procs} processes)`);
      await t.nav(path.dirname(d));
    },
  },
];
