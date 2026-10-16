// One modal question at a time, rendered by Dialog.svelte.

export interface Ask {
  title: string;
  message: string;
  ok: string;
  cancel: string;
  /** Destructive actions get a red OK button. */
  danger: boolean;
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
