<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app, navigate, pinAdd, pinRemove, shellIcon } from "./store.svelte";
  import { settings, menuOn, anyMenuOn, extraOn, rememberExtras } from "./settings.svelte";

  interface ShellItem {
    id: number;
    text: string;
    verb: string | null;
    sep: boolean;
    disabled: boolean;
    sub: ShellItem[] | null;
  }

  function fmtBytes(v: number): string {
    const TB = 2 ** 40;
    const GB = 2 ** 30;
    if (v >= TB) return (v / TB).toFixed(2) + " TB";
    const gb = v / GB;
    return (gb >= 10 ? Math.round(gb).toString() : gb.toFixed(1)) + " GB";
  }

  let ctx = $state<{ x: number; y: number; path: string } | null>(null);
  let ctxItems = $state<ShellItem[] | null>(null);
  let openSub = $state(-1);
  let ctxSeq = 0;

  // Rendered natively and instantly; shell duplicates of these are dropped.
  const NATIVE_VERBS = new Set(["open", "opennewwindow", "properties", "pintohome", "unpinfromhome"]);
  function filterExtras(items: ShellItem[]): ShellItem[] {
    const out: ShellItem[] = [];
    for (const it of items) {
      if (it.sep) {
        if (out.length && !out[out.length - 1].sep) out.push(it);
        continue;
      }
      if (NATIVE_VERBS.has((it.verb ?? "").toLowerCase())) continue;
      out.push(it);
    }
    while (out.length && out[out.length - 1].sep) out.pop();
    rememberExtras(out.filter((it) => !it.sep).map((it) => it.text));
    return out.filter((it) => it.sep || extraOn(it.text));
  }

  const DRIVE_IDS = ["drive.open", "drive.pin", "drive.properties"];

  function onContext(ev: MouseEvent, path: string) {
    ev.preventDefault();
    if (!anyMenuOn(DRIVE_IDS) && !settings.shellExtras) return;
    ctx = {
      x: Math.min(ev.clientX, window.innerWidth - 300),
      y: Math.min(ev.clientY, window.innerHeight - 420),
      path,
    };
    ctxItems = null;
    openSub = -1;
    const seq = ++ctxSeq;
    if (!settings.shellExtras) {
      ctxItems = [];
      return;
    }
    // dir "" = absolute-path item: the drive itself, not its contents.
    // Native entries show instantly; extension entries stream in when ready.
    invoke<ShellItem[]>("shell_menu", { dir: "", names: [path] })
      .then((items) => {
        if (seq === ctxSeq && ctx) ctxItems = filterExtras(items);
      })
      .catch(() => {
        if (seq === ctxSeq) ctxItems = [];
      });
  }

  async function shellInvoke(it: ShellItem) {
    const path = ctx?.path ?? "";
    ctx = null;
    const v = (it.verb ?? "").toLowerCase();
    try {
      if (v === "open") return void navigate(path);
      if (v === "properties") return void (await invoke("show_properties_path", { path }));
      if (v === "pintohome") return void pinAdd(path);
      if (v === "unpinfromhome") return void pinRemove(path);
      await invoke("shell_menu_invoke", { id: it.id });
    } catch (e) {
      app.status = String(e);
    }
  }
</script>

<svelte:window onmousedown={(e) => { if (ctx && !(e.target as HTMLElement).closest(".drivemenu")) ctx = null; }} />

<div class="drivespane">
  <div class="section">Devices and drives ({app.drives.length})</div>
  <div class="tiles">
    {#each app.drives as d (d.path)}
      {@const used = d.total ? (d.total - d.free) / d.total : 0}
      <button
        class="drivetile"
        ondblclick={() => navigate(d.path)}
        oncontextmenu={(e) => onContext(e, d.path)}
        title={d.path}
      >
        {#if shellIcon(d.path, 48)}
          <img class="dimg" src={shellIcon(d.path, 48)} alt="" width="48" height="48" />
        {:else}
          <span class="glyph icon">&#xEDA2;</span>
        {/if}
        <span class="info">
          <span class="name">{d.name}</span>
          <span class="bar" class:low={d.free < d.total * 0.1}>
            <span class="fill" style="width: {Math.min(100, used * 100)}%"></span>
          </span>
          <span class="free">{fmtBytes(d.free)} free of {fmtBytes(d.total)}</span>
        </span>
      </button>
    {/each}
  </div>
</div>

{#if ctx}
  <div class="menu drivemenu" style="left:{ctx.x}px; top:{ctx.y}px">
    {#if menuOn("drive.open")}
      <button class="menu-item" onclick={() => { const p = ctx!.path; ctx = null; navigate(p); }}>
        <span class="glyph">&#xE8E5;</span> Open
      </button>
    {/if}
    {#if menuOn("drive.pin")}
      <button class="menu-item" onclick={() => { const p = ctx!.path; ctx = null; pinAdd(p); }}>
        <span class="glyph">&#xE840;</span> Pin to Quick access
      </button>
    {/if}
    {#if menuOn("drive.properties")}
      <button class="menu-item" onclick={() => { const p = ctx!.path; ctx = null; invoke("show_properties_path", { path: p }).catch((e) => (app.status = String(e))); }}>
        <span class="glyph">&#xE90F;</span> Properties
      </button>
    {/if}
    {#if ctxItems === null}
      <div class="menu-sep"></div>
      <div class="loading">Loading…</div>
    {:else if ctxItems.length}
      <div class="menu-sep"></div>
      <div class="dscroll">
        {#each ctxItems as it, i (i)}
          {#if it.sep}
            <div class="menu-sep"></div>
          {:else}
            <div class="subwrap" role="none" onmouseenter={() => (openSub = it.sub ? i : -1)}>
              <button class="menu-item wide" disabled={it.disabled} onclick={() => !it.sub && shellInvoke(it)}>
                <span class="mtext">{it.text}</span>
                {#if it.sub}<span class="glyph subchev">&#xE76C;</span>{/if}
              </button>
              {#if it.sub && openSub === i}
                <div class="menu submenu">
                  {#each it.sub as s, sidx (sidx)}
                    {#if s.sep}
                      <div class="menu-sep"></div>
                    {:else}
                      <button class="menu-item wide" disabled={s.disabled} onclick={() => shellInvoke(s)}>
                        <span class="mtext">{s.text}</span>
                      </button>
                    {/if}
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .drivemenu {
    min-width: 240px;
    max-width: 320px;
  }
  .subwrap {
    position: relative;
  }
  .menu-item.wide {
    justify-content: space-between;
    gap: 8px;
  }
  .mtext {
    flex: 1;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .subchev {
    font-size: 10px;
    color: var(--text-3);
  }
  .submenu {
    position: absolute;
    left: calc(100% - 4px);
    top: -4px;
    max-height: 50vh;
    overflow-y: auto;
  }
  .dscroll {
    max-height: 60vh;
    overflow-y: auto;
  }
  .loading {
    padding: 10px 14px;
    font-size: 12px;
    color: var(--text-3);
  }
  .drivespane {
    flex: 1;
    overflow-y: auto;
    padding: 12px 16px;
  }
  .section {
    font-size: 13px;
    color: var(--text-2);
    padding: 4px 0 10px 4px;
  }
  .tiles {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .drivetile {
    width: 300px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-radius: 4px;
    text-align: left;
  }
  .drivetile:hover {
    background: var(--hover);
  }
  .drivetile .icon {
    font-size: 40px;
    color: var(--text-2);
    flex-shrink: 0;
  }
  .dimg {
    flex-shrink: 0;
  }
  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .name {
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .bar {
    height: 12px;
    background: var(--input-bg);
    border: 1px solid var(--stroke-strong);
    border-radius: 2px;
    overflow: hidden;
  }
  .bar .fill {
    display: block;
    height: 100%;
    background: var(--accent, #0078d4);
  }
  .bar.low .fill {
    background: var(--close-hover);
  }
  .free {
    font-size: 12px;
    color: var(--text-3);
    white-space: nowrap;
  }
</style>
