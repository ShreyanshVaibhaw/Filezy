<!-- A modal dialog on the native <dialog>. showModal() puts it in the top layer
     and makes the page behind it inert, so Tab, clicks and screen readers stay
     inside; Escape arrives as the cancel event. Render it only while open:
     focus goes back to where it was when it closes. -->
<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    onclose,
    labelledby,
    describedby,
    role,
    width,
    busy,
    onkeydown,
    children,
  }: {
    /** Escape, or anything else that dismisses it. */
    onclose: () => void;
    labelledby: string;
    describedby?: string;
    role?: "alertdialog";
    width: number;
    /** Still working: screen readers wait for it to settle. */
    busy?: boolean;
    /** Keys inside the dialog, before they're kept from the rest of the app. */
    onkeydown?: (ev: KeyboardEvent) => void;
    children: Snippet;
  } = $props();

  const returnFocus = document.activeElement;

  /// Tab past the last control comes back to the first, and Shift+Tab the
  /// other way. Left alone, the WebView moves focus to the inert page's body.
  function wrapTab(ev: KeyboardEvent) {
    const dlg = ev.currentTarget as HTMLElement;
    const stops = [...dlg.querySelectorAll<HTMLElement>("button, input, select, textarea, [tabindex]")].filter(
      (el) => el.tabIndex >= 0 && !(el as HTMLButtonElement).disabled && el.getClientRects().length > 0,
    );
    const edge = ev.shiftKey ? stops[0] : stops.at(-1);
    if (!stops.length || document.activeElement === edge || !dlg.contains(document.activeElement)) {
      ev.preventDefault();
      (ev.shiftKey ? stops.at(-1) : stops[0])?.focus();
    }
  }

  function modal(node: HTMLDialogElement) {
    node.showModal();
    return {
      destroy() {
        (returnFocus as HTMLElement | null)?.focus?.();
      },
    };
  }
</script>

<dialog
  use:modal
  class="dlg"
  style:width="min({width}px, calc(100vw - 32px))"
  {role}
  aria-labelledby={labelledby}
  aria-describedby={describedby}
  aria-busy={busy}
  oncancel={(ev) => {
    ev.preventDefault();
    onclose();
  }}
  onkeydown={(ev) => {
    onkeydown?.(ev);
    if (ev.key === "Tab") wrapTab(ev);
    // The dialog owns the keyboard: no shortcut reaches the file list behind it.
    ev.stopPropagation();
  }}
>
  {@render children()}
</dialog>
