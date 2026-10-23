<script lang="ts">
  import { msg } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { app, sel, fmtSizeAuto, selRef, setView, stopSearch } from "./store.svelte";

  let selSize = $state(0);
  const selN = $derived(app.allSelected ? app.total : sel.size);

  let statsSeq = 0;
  $effect(() => {
    void sel.size;
    void app.allSelected;
    void app.rev;
    const seq = ++statsSeq;
    if (!selN) {
      selSize = 0;
      return;
    }
    invoke<{ count: number; files: number; bytes: number }>("selection_stats", { sel: selRef() })
      .then((s) => seq === statsSeq && (selSize = s.bytes))
      .catch(() => {});
  });

  // What a finished or running search covered, said plainly.
  const searchNote = $derived.by(() => {
    const s = app.search;
    if (!s) return "";
    // What covered the search, and what it couldn't: said while it runs, too.
    const covered = s.provider === "index" ? "Results from the Windows index" : s.note;
    if (app.searching) return ["Searching…", covered].filter(Boolean).join(". ");
    const parts: string[] = [];
    if (s.stopped) parts.push("Search stopped; these are the results found so far");
    if (s.capped) parts.push(`Showing the first ${app.total.toLocaleString()} results`);
    if (s.skipped) parts.push(`${s.skipped.toLocaleString()} folder${s.skipped === 1 ? "" : "s"} couldn't be searched`);
    if (covered) parts.push(covered);
    return parts.join(". ");
  });
</script>

<div class="statusbar">
  <span class="stat">{app.total.toLocaleString()} {msg("item")}{app.total === 1 ? "" : "s"}</span>
  {#if selN}
    <span class="statsep"></span>
    <span class="stat">
      {selN.toLocaleString()} {msg("item")}{selN > 1 ? "s" : ""} {msg("selected")}
      {#if selSize}&nbsp;{fmtSizeAuto(selSize)}{/if}
    </span>
  {/if}
  {#if searchNote}
    <span class="statsep"></span>
    <span class="stat" class:warn={!app.searching && (app.search?.capped || app.search?.skipped)}>{searchNote}</span>
  {/if}
  {#if app.searching}
    <button class="stopbtn" onclick={stopSearch} title={msg("Stop searching")} aria-label={msg("Stop searching")}>{msg("Stop")}</button>
  {/if}
  {#if !app.live && !app.search}
    <span class="statsep"></span>
    <span class="stat" title={msg("Windows doesn't report changes for this location")}>{msg("Doesn't refresh on its own. Press F5 to update.")}</span>
  {/if}
  <div class="fill"></div>
  <button class="viewtoggle" class:active={app.view === "details"} title={msg("Details view")} aria-label={msg("Details view")} onclick={() => setView("details")}>
    <span class="glyph" aria-hidden="true">&#xE9D5;</span>
  </button>
  <button class="viewtoggle" class:active={app.view === "icons_l"} title={msg("Large icons view")} aria-label={msg("Large icons view")} onclick={() => setView("icons_l")}>
    <span class="glyph" aria-hidden="true">&#xE8A9;</span>
  </button>
</div>

<style>
  .statusbar {
    height: 32px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    border-top: 1px solid var(--stroke);
    flex-shrink: 0;
  }
  .stat {
    font-size: 12px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .stat.warn {
    color: var(--text-2);
  }
  .statsep {
    width: 1px;
    height: 14px;
    background: var(--stroke-strong);
    flex-shrink: 0;
  }
  .stopbtn {
    height: 22px;
    padding: 0 10px;
    font-size: 12px;
    border: 1px solid var(--stroke-strong);
    border-radius: 4px;
    flex-shrink: 0;
  }
  .stopbtn:hover {
    background: var(--hover);
  }
  .fill {
    flex: 1;
  }
  .viewtoggle {
    width: 32px;
    height: 24px;
    display: grid;
    place-items: center;
    flex-shrink: 0;
  }
  .viewtoggle .glyph {
    font-size: 14px;
    color: var(--text-3);
  }
  .viewtoggle.active .glyph {
    color: var(--accent);
  }
  .viewtoggle:hover:not(:disabled) {
    background: var(--hover);
  }
</style>
