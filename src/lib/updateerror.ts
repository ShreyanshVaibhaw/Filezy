// The updater plugin's errors in plain words, apart from updater.svelte.ts so
// the node tests can load it.

/// What went wrong with an update, said plainly; anything unknown keeps its own text.
export function explainUpdateError(e: unknown): string {
  const raw = String(e).replace(/`/g, "").trim();
  if (/release JSON|error sending request|dns|connect/i.test(raw)) return "Couldn't get update information from the server. Check your connection and try again later.";
  const status = /status: (\d{3})/i.exec(raw);
  if (status) return `The update couldn't be downloaded (the server answered ${status[1]}). Try again later.`;
  if (/signature/i.test(raw)) return "The downloaded update failed its signature check, so it wasn't installed.";
  return raw;
}
