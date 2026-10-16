<script lang="ts">
  import { dialog } from "./ask.svelte";

  let okBtn: HTMLButtonElement | undefined = $state();
  let returnFocus: Element | null = null;

  $effect(() => {
    if (dialog.ask) {
      returnFocus = document.activeElement;
      queueMicrotask(() => okBtn?.focus());
    }
  });

  function answer(ok: boolean) {
    dialog.ask?.resolve(ok);
    (returnFocus as HTMLElement | null)?.focus?.();
  }

  function onKeydown(ev: KeyboardEvent) {
    if (!dialog.ask) return;
    // The dialog owns the keyboard while open; nothing reaches the file list.
    ev.stopPropagation();
    if (ev.key === "Escape") {
      ev.preventDefault();
      answer(false);
    } else if (ev.key === "Tab") {
      // Keep focus on the two buttons.
      const btns = [...document.querySelectorAll<HTMLButtonElement>(".dlg button")];
      const i = btns.indexOf(document.activeElement as HTMLButtonElement);
      btns[(i + (ev.shiftKey ? btns.length - 1 : 1)) % btns.length]?.focus();
      ev.preventDefault();
    }
  }
</script>

<svelte:window onkeydowncapture={onKeydown} />

{#if dialog.ask}
  <div class="scrim" role="presentation">
    <div class="dlg" role="alertdialog" aria-modal="true" aria-labelledby="dlg-title" aria-describedby="dlg-msg">
      <h2 id="dlg-title">{dialog.ask.title}</h2>
      <p id="dlg-msg">{dialog.ask.message}</p>
      <div class="btns">
        <button class="btn primary" class:danger={dialog.ask.danger} bind:this={okBtn} onclick={() => answer(true)}>
          {dialog.ask.ok}
        </button>
        <button class="btn" onclick={() => answer(false)}>{dialog.ask.cancel}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 2000;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.3);
  }
  .dlg {
    width: min(440px, calc(100vw - 32px));
    background: var(--menu-bg);
    border: 1px solid var(--stroke-strong);
    border-radius: 8px;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.4);
    padding: 20px 24px 0;
    overflow: hidden;
  }
  h2 {
    font-size: 18px;
    font-weight: 600;
    margin-bottom: 10px;
  }
  p {
    font-size: 14px;
    line-height: 1.45;
    color: var(--text-2);
    white-space: pre-line;
    margin-bottom: 20px;
    user-select: text;
  }
  .btns {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin: 0 -24px;
    padding: 16px 24px;
    background: var(--surface);
    border-top: 1px solid var(--stroke);
  }
  .btn {
    min-width: 96px;
    height: 32px;
    padding: 0 12px;
    border: 1px solid var(--stroke-strong);
    border-radius: 4px;
    background: var(--input-bg);
  }
  .btn:hover {
    background: var(--surface-hover);
  }
  .btn:focus-visible {
    outline: 2px solid var(--text);
    outline-offset: 1px;
  }
  .btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--menu-bg);
  }
  .btn.primary.danger {
    background: var(--close-hover);
    border-color: var(--close-hover);
    color: #fff;
  }
</style>
