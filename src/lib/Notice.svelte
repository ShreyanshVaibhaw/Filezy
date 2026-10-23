<script lang="ts">
  import { msg } from "./i18n.svelte";
  import { app } from "./store.svelte";
  import { baseName } from "./paths";
  import { activity, dismissNotice } from "./ops.svelte";

  // The operation report behind the message, if the message is one.
  const cur = $derived(activity.current?.title === app.status ? activity.current : null);
  // Messages set through say() are information; anything else written to
  // app.status is an error from a failed command.
  const tone = $derived(cur?.tone ?? (app.status === app.statusInfo ? "info" : "error"));
  const icon = $derived(tone === "error" ? "" : tone === "warn" ? "" : "");

  // Informational messages clear themselves; problems stay until dismissed.
  $effect(() => {
    const shown = app.status;
    if (!shown || tone === "error" || tone === "warn") return;
    // ponytail: fixed 6 s; pause while hovered if people miss the Retry button
    const t = setTimeout(() => {
      if (app.status === shown) dismissNotice();
    }, 6000);
    return () => clearTimeout(t);
  });

  async function retry() {
    const r = cur?.retry;
    dismissNotice();
    await r?.();
  }

  /** Windows' error code, as Explorer's dialogs print it: 0x80070005. */
  const hex = (hr: number) => "0x" + (hr >>> 0).toString(16).toUpperCase().padStart(8, "0");
  const stateText: Record<string, string> = {
    skipped: "Skipped",
    failed: "Failed",
    unattempted: "Not attempted",
  };
</script>

<svelte:window
  onmousedown={(e) => {
    if (activity.open && !(e.target as HTMLElement).closest(".activity, .detailsbtn")) activity.open = false;
  }}
  onkeydown={(e) => {
    if (activity.open && e.key === "Escape") activity.open = false;
  }}
/>

{#if app.status}
  <div class="notice {tone}" role={tone === "error" ? "alert" : "status"}>
    <span class="glyph icon" aria-hidden="true">{icon}</span>
    <span class="text" title={app.status}>{app.status}</span>
    {#if cur?.problems.length}
      <button class="act detailsbtn" onclick={() => (activity.open = !activity.open)}>{msg("Details")}</button>
    {/if}
    {#if cur?.retry}
      <button class="act" onclick={retry}>{msg("Retry")}</button>
    {/if}
    <button class="close" aria-label={msg("Dismiss message")} title={msg("Dismiss")} onclick={dismissNotice}>
      <span class="glyph" aria-hidden="true">&#xE8BB;</span>
    </button>
  </div>
{/if}

{#if activity.open}
  <div class="menu activity" role="dialog" aria-label={msg("Recent operations")}>
    <div class="ahead">{msg("Recent operations")}</div>
    {#each activity.list as a (a.id)}
      <div class="aitem">
        <div class="atitle {a.tone}">{a.title}</div>
        {#each a.problems.slice(0, 50) as p (p.src)}
          <div class="aprob" title={p.src}>
            <span class="pname">{baseName(p.src)}</span>
            <span class="pstate">{stateText[p.state] ?? p.state}{p.error ? `: ${p.error}` : ""}{p.hr < 0 ? ` (${hex(p.hr)})` : ""}</span>
          </div>
        {/each}
        {#if a.problems.length > 50}
          <div class="aprob more">{msg("…and")} {a.problems.length - 50} {msg("more")}</div>
        {/if}
      </div>
    {:else}
      <div class="aitem empty">{msg("No operations yet.")}</div>
    {/each}
  </div>
{/if}

<style>
  .notice {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 40px;
    padding: 4px 6px 4px 14px;
    border-top: 1px solid var(--stroke);
    flex-shrink: 0;
    font-size: 13px;
  }
  .notice.error {
    background: color-mix(in srgb, #c42b1c 16%, transparent);
  }
  .notice.warn {
    background: color-mix(in srgb, #9d5d00 18%, transparent);
  }
  .notice.info,
  .notice.ok {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .icon {
    font-size: 16px;
    flex-shrink: 0;
  }
  .error .icon {
    color: var(--danger);
  }
  .warn .icon {
    color: var(--caution);
  }
  .info .icon,
  .ok .icon {
    color: var(--accent);
  }
  .text {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    user-select: text;
  }
  .act {
    height: 28px;
    padding: 0 12px;
    font-size: 13px;
    border: 1px solid var(--stroke-strong);
    border-radius: 4px;
    flex-shrink: 0;
  }
  .act:hover {
    background: var(--hover);
  }
  .close {
    width: 32px;
    height: 28px;
    display: grid;
    place-items: center;
    flex-shrink: 0;
  }
  .close .glyph {
    font-size: 10px;
    color: var(--text-2);
  }
  .close:hover {
    background: var(--hover);
  }
  .activity {
    right: 12px;
    bottom: 48px;
    width: min(460px, calc(100vw - 24px));
    max-height: 50vh;
    overflow-y: auto;
    padding: 6px;
  }
  .ahead {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-2);
    padding: 6px 8px;
  }
  .aitem {
    padding: 6px 8px;
    border-radius: 4px;
  }
  .aitem + .aitem {
    border-top: 1px solid var(--stroke);
  }
  .atitle {
    font-size: 13px;
  }
  .atitle.error {
    color: var(--danger);
  }
  .aprob {
    display: flex;
    gap: 8px;
    font-size: 12px;
    color: var(--text-3);
    padding: 2px 0 0 12px;
  }
  .pname {
    flex-shrink: 0;
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-2);
  }
  .pstate {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .empty {
    font-size: 13px;
    color: var(--text-3);
  }
</style>
