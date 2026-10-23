<!-- Shell menu entries with their submenus, any depth. A submenu is a fixed
     flyout next to its entry, so the menu's own scrolling can't clip it, and it
     opens to the left when there is no room on the right. -->
<script lang="ts">
  import type { ShellItem } from "./shellmenu";
  import { menuKeys } from "./menukeys";
  import ShellItems from "./ShellItems.svelte";

  let { items, run }: { items: ShellItem[]; run: (it: ShellItem) => void } = $props();

  let open = $state(-1);
  // When any entry can be checked, every entry keeps the check's column, so
  // the texts line up as in Windows' menus.
  const checks = $derived(items.some((it) => it.checked !== undefined));
  /** The open entry's edges, to place its flyout. */
  let edge = { left: 0, right: 0, top: 0 };

  function enter(ev: MouseEvent, idx: number, it: ShellItem) {
    if (it.sub) {
      const r = (ev.currentTarget as HTMLElement).getBoundingClientRect();
      edge = { left: r.left, right: r.right, top: r.top };
    }
    open = it.sub ? idx : -1;
  }

  function place(node: HTMLElement) {
    const w = node.offsetWidth;
    const h = node.offsetHeight;
    const left = edge.right - 4 + w <= innerWidth - 4 ? edge.right - 4 : Math.max(4, edge.left + 4 - w);
    node.style.left = `${left}px`;
    node.style.top = `${Math.max(4, Math.min(edge.top - 4, innerHeight - 4 - h))}px`;
  }
</script>

{#each items as it, idx (idx)}
  {#if it.sep}
    <div class="menu-sep"></div>
  {:else}
    <div class="subwrap" role="none" onmouseenter={(ev) => enter(ev, idx, it)}>
      <button
        class="menu-item wide"
        disabled={it.disabled}
        onclick={() => !it.sub && run(it)}
        role={it.checked === undefined ? undefined : "menuitemradio"}
        aria-checked={it.checked}
      >
        {#if checks}<span class="glyph tick" aria-hidden="true">{it.checked ? "\uE73E" : ""}</span>{/if}
        <span class="mtext">{it.text}</span>
        {#if it.sub}<span class="glyph subchev" aria-hidden="true">&#xE76C;</span>{/if}
      </button>
      {#if it.sub && open === idx}
        <div class="menu submenu" use:place use:menuKeys={() => (open = -1)}>
          <ShellItems items={it.sub} {run} />
        </div>
      {/if}
    </div>
  {/if}
{/each}

<style>
  .tick {
    width: 16px;
    flex-shrink: 0;
  }
  /* Fixed, placed by place(), so the parent menu's scrolling can't clip it. */
  .menu.submenu {
    position: fixed;
  }
</style>
