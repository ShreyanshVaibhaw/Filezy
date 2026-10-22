// One modal question at a time, rendered by Dialog.svelte.

export interface Ask {
  title: string;
  message: string;
  ok: string;
  cancel: string;
  /** Destructive actions get a red OK button. */
  danger: boolean;
  /** Set for a question that takes a line of text (prompt()). */
  value?: string;
  resolve: (ok: boolean) => void;
}

export const dialog = $state<{ ask: Ask | null }>({ ask: null });

export function confirm(
  title: string,
  message: string,
  opts: { ok?: string; cancel?: string; danger?: boolean } = {},
): Promise<boolean> {
  dialog.ask?.resolve(false);
  return new Promise((resolve) => {
    dialog.ask = {
      title,
      message,
      ok: opts.ok ?? "OK",
      cancel: opts.cancel ?? "Cancel",
      danger: opts.danger ?? false,
      resolve: (ok) => {
        dialog.ask = null;
        resolve(ok);
      },
    };
  });
}

/** Ask for a line of text; null when cancelled or left blank. */
export async function prompt(title: string, message: string, initial = "", ok = "Save"): Promise<string | null> {
  dialog.ask?.resolve(false);
  const answered = await new Promise<boolean>((resolve) => {
    dialog.ask = {
      title,
      message,
      ok,
      cancel: "Cancel",
      danger: false,
      value: initial,
      resolve: (yes) => {
        dialog.ask = null;
        resolve(yes);
      },
    };
  });
  return answered ? (last.trim() || null) : null;
}
// The prompt's text as the dialog last had it (the ask object is gone by then).
let last = "";
export function setPromptText(v: string) {
  last = v;
}
