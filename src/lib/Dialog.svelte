<script lang="ts">
  import { dialog, setPromptText } from "./ask.svelte";
  import Modal from "./Modal.svelte";

  let okBtn: HTMLButtonElement | undefined = $state();
  let field: HTMLInputElement | undefined = $state();
  let text = $state("");

  $effect(() => {
    if (dialog.ask) {
      text = dialog.ask.value ?? "";
      queueMicrotask(() => {
        if (field) {
          field.focus();
          field.select();
        } else okBtn?.focus();
      });
    }
  });

  function answer(ok: boolean) {
    setPromptText(text);
    dialog.ask?.resolve(ok);
  }
</script>

{#if dialog.ask}
  <Modal
    width={440}
    role="alertdialog"
    labelledby="dlg-title"
    describedby="dlg-msg"
    onclose={() => answer(false)}
    onkeydown={(ev) => {
      if (ev.key === "Enter" && ev.target === field) {
        ev.preventDefault();
        answer(true);
      }
    }}
  >
    <h2 id="dlg-title">{dialog.ask.title}</h2>
    <p id="dlg-msg">{dialog.ask.message}</p>
    {#if dialog.ask.value !== undefined}
      <input class="field" bind:this={field} bind:value={text} spellcheck="false" aria-labelledby="dlg-msg" />
    {/if}
    <div class="btns">
      <button class="btn primary" class:danger={dialog.ask.danger} bind:this={okBtn} onclick={() => answer(true)}>
        {dialog.ask.ok}
      </button>
      <button class="btn" onclick={() => answer(false)}>{dialog.ask.cancel}</button>
    </div>
  </Modal>
{/if}

<style>
  h2 {
    margin-bottom: 10px;
  }
  .field {
    width: 100%;
    height: 32px;
    margin: 8px 0 4px;
    padding: 0 10px;
    background: var(--input-bg);
    border: 1px solid var(--stroke);
    border-bottom: 2px solid var(--accent);
    border-radius: 4px;
    user-select: text;
  }
  p {
    font-size: 14px;
    line-height: 1.45;
    color: var(--text-2);
    white-space: pre-line;
    margin-bottom: 4px;
    user-select: text;
  }
</style>
