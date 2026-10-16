<script lang="ts">
  import { menuKeys, fitMenu } from "./menukeys";
  import { invoke } from "@tauri-apps/api/core";
  import { app, navigate, pinAdd, pinRemove, shellIcon, loadDrives, type Drive } from "./store.svelte";
  import { settings, menuOn, anyMenuOn } from "./settings.svelte";
  import ShellItems from "./ShellItems.svelte";
  import { filterExtras, followMenu, type ShellItem, type ShellMenu } from "./shellmenu";

  /// What a drive that can't be read shows instead of its free space.
  function stateText(d: Drive): string {
    switch (d.state) {
      case "empty":
        return d.kind === "optical" ? "No disc" : "No media";
      case "offline":
        return d.remote ? `Disconnected from ${d.remote}` : "Disconnected";
      case "slow":
        return "Not responding";
      default:
        return d.note ?? "Can't be read";
    }
  }

  function fmtBytes(v: number): string {
    const TB = 2 ** 40;
    const GB = 2 ** 30;
    if (v >= TB) return (v / TB).toFixed(2) + " TB";
    const gb = v / GB;
    return (gb >= 10 ? Math.round(gb).toString() : gb.toFixed(1)) + " GB";
  }

  let ctx = $state<{ x: number; y: number; at: { x: number; y: number }; path: string } | null>(null);
  // The drive the menu is for, for its Eject and Disconnect entries.
  const ctxDrive = $derived(ctx ? app.drives.find((d) => d.path === ctx?.path) : undefined);
  let ctxItems = $state<ShellItem[] | null>(null);
  let ctxNote = $state("");
  let ctxSession = 0;
  let ctxSeq = 0;

  // Rendered natively and instantly; shell duplicates of these are dropped,
  // and Windows' own menu hands them back to run here.
  const NATIVE_VERBS = new Set(["open", "opennewwindow", "opennewprocess", "properties", "pintohome", "unpinfromhome", "eject", "disconnect"]);

  const DRIVE_IDS = ["drive.open", "drive.pin", "drive.eject", "drive.disconnect", "drive.properties", "drive.moreoptions"];

  async function eject(path: string) {
    try {
      await invoke("eject", { path });
    } catch (e) {
      app.status = String(e);
    }
    await loadDrives();
  }

  async function disconnect(path: string) {
    try {
      await invoke("disconnect_drive", { path });
    } catch (e) {
      app.status = String(e);
    }
    await loadDrives();
  }

  function onContext(ev: MouseEvent, path: string) {
    ev.preventDefault();
    if (ev.shiftKey && menuOn("drive.moreoptions")) return void moreOptions(path, { x: ev.clientX, y: ev.clientY }, true);
    if (!anyMenuOn(DRIVE_IDS) && !settings.shellExtras) return;
    ctx = {
      x: ev.clientX,
      y: ev.clientY,
      at: { x: ev.clientX, y: ev.clientY },
      path,
    };
    ctxItems = null;
    ctxNote = "";
    const seq = ++ctxSeq;
    if (!settings.shellExtras) {
      ctxItems = [];
      return;
    }
    // Native entries show instantly; extension entries stream in when ready.
    invoke<ShellMenu>("shell_menu_paths", { paths: [path] })
      .then((m) => {
        if (seq !== ctxSeq || !ctx) return;
        ctxSession = m.session;
        ctxNote = m.note ?? "";
        ctxItems = filterExtras(m.items, NATIVE_VERBS);
        followMenu(m, () => seq === ctxSeq && !!ctx, (items) => (ctxItems = filterExtras(items, NATIVE_VERBS)));
      })
      .catch((e) => {
        if (seq !== ctxSeq) return;
        if (String(e) !== "superseded") ctxNote = String(e);
        ctxItems = [];
      });
  }

  /// The drive verbs this page runs itself. False when it isn't one.
  async function runVerb(v: string, path: string): Promise<boolean> {
    switch (v) {
      case "open":
        await navigate(path);
        return true;
      case "opennewwindow":
      case "opennewprocess":
        await invoke("new_window", { path });
        return true;
      case "properties":
        await invoke("show_properties", { path });
        return true;
      case "pintohome":
        await pinAdd(path);
        return true;
      case "unpinfromhome":
        await pinRemove(path);
        return true;
    }
    return false;
  }

  async function shellInvoke(it: ShellItem) {
    const path = ctx?.path ?? "";
    ctx = null;
    try {
      if (await runVerb((it.verb ?? "").toLowerCase(), path)) return;
      await invoke("shell_menu_invoke", { session: ctxSession, id: it.id });
    } catch (e) {
      app.status = String(e);
    }
  }

  /// Windows' own menu for the drive ("Show more options").
  async function moreOptions(path: string, at: { x: number; y: number }, extended = false) {
    ctx = null;
    // Closing our menu hands the focus back to the page; let that land first,
    // or it arrives while Windows' menu is open and closes it.
    await new Promise((r) => setTimeout(r, 60));
    const s = window.devicePixelRatio || 1;
    try {
      const verb = await invoke<string | null>("shell_menu_native", {
        paths: [path],
        x: Math.round(at.x * s),
        y: Math.round(at.y * s),
        extended,
        handled: [...NATIVE_VERBS],
      });
      if (verb) await runVerb(verb, path);
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
        data-drop={d.path}
        data-drop-label={d.name}
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
          {#if d.state === "ready"}
            <span class="bar" class:low={d.free < d.total * 0.1}>
              <span class="fill" style="width: {Math.min(100, used * 100)}%"></span>
            </span>
            <span class="free">{fmtBytes(d.free)} free of {fmtBytes(d.total)}{d.read_only ? " · Read-only" : ""}</span>
          {:else}
            <span class="free state" title={d.note ?? ""}>{stateText(d)}</span>
          {/if}
        </span>
      </button>
    {/each}
  </div>
</div>

{#if ctx}
  <div class="menu drivemenu" style="left:{ctx.x}px; top:{ctx.y}px" use:menuKeys={() => (ctx = null)} use:fitMenu={ctx}>
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
    {#if ctxDrive?.ejectable && menuOn("drive.eject")}
      <button class="menu-item" onclick={() => { const p = ctx!.path; ctx = null; eject(p); }}>
        <span class="glyph">&#xF847;</span> Eject
      </button>
    {/if}
    {#if ctxDrive?.kind === "network" && menuOn("drive.disconnect")}
      <button class="menu-item" onclick={() => { const p = ctx!.path; ctx = null; disconnect(p); }}>
        <span class="glyph">&#xE8CD;</span> Disconnect
      </button>
    {/if}
    {#if menuOn("drive.properties")}
      <button class="menu-item" onclick={() => { const p = ctx!.path; ctx = null; invoke("show_properties", { path: p }).catch((e) => (app.status = String(e))); }}>
        <span class="glyph">&#xE90F;</span> Properties
      </button>
    {/if}
    {#if ctxItems === null}
      <div class="menu-sep"></div>
      <div class="loading">Loading…</div>
    {:else if ctxItems.length}
      <div class="menu-sep"></div>
      <div class="extras">
        <ShellItems items={ctxItems} run={shellInvoke} />
      </div>
    {:else if ctxNote && settings.shellExtras}
      <div class="menu-sep"></div>
      <div class="menunote" role="note">{ctxNote}</div>
    {/if}
    {#if menuOn("drive.moreoptions")}
      <div class="menu-sep"></div>
      <button class="menu-item" onclick={() => ctx && moreOptions(ctx.path, ctx.at)}>
        <span class="glyph">&#xE712;</span> Show more options
      </button>
    {/if}
  </div>
{/if}

<style>
  .drivemenu {
    min-width: 240px;
    max-width: 320px;
  }
  .extras {
    max-height: 45vh;
    overflow-y: auto;
  }
  .loading,
  .menunote {
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
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
