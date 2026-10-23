// Minimal Chrome DevTools Protocol client over Node's built-in WebSocket.
// WebView2 exposes the protocol when the process starts with
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>.
import fs from "node:fs";
import net from "node:net";
import path from "node:path";

/** A TCP port nothing is listening on right now. */
export function freePort() {
  return new Promise((resolve) => {
    const s = net.createServer().listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

/**
 * Environment for a Filezy launched against a throwaway profile in `root`:
 * DevTools on `port`, and WebView2 storage, %APPDATA% and %TEMP% all under
 * `root`, so nothing touches the user's own Filezy data.
 */
export function profileEnv(root, port) {
  const temp = path.join(root, "temp");
  fs.mkdirSync(temp, { recursive: true });
  return {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    WEBVIEW2_USER_DATA_FOLDER: path.join(root, "webview"),
    APPDATA: path.join(root, "appdata"),
    TEMP: temp,
    TMP: temp,
  };
}

/** Connect to a page; `pick` chooses among the pages (default: the first). */
export async function connect(port, timeoutMs = 30000, pick = (pages) => pages[0]) {
  const deadline = Date.now() + timeoutMs;
  let target;
  while (!target) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      target = pick(list.filter((t) => t.type === "page"));
    } catch {
      // the browser process is still starting
    }
    if (!target) {
      if (Date.now() > deadline) throw new Error(`no DevTools page on port ${port}`);
      await sleep(200);
    }
  }

  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = () => reject(new Error("DevTools socket failed to open"));
  });

  let nextId = 0;
  const pending = new Map();
  const listeners = new Map();
  ws.onmessage = (msg) => {
    const m = JSON.parse(msg.data);
    if (m.id) {
      const p = pending.get(m.id);
      pending.delete(m.id);
      if (m.error) p?.reject(new Error(`${m.error.message} (${p.method})`));
      else p?.resolve(m.result);
    } else {
      for (const f of listeners.get(m.method) ?? []) f(m.params);
    }
  };
  ws.onclose = () => {
    for (const p of pending.values()) p.reject(new Error("DevTools socket closed"));
    pending.clear();
  };

  function send(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = ++nextId;
      pending.set(id, { resolve, reject, method });
      ws.send(JSON.stringify({ id, method, params }));
    });
  }

  /** Evaluate an expression in the page; promises are awaited, values come back as JSON. */
  async function ev(expression) {
    const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) {
      const d = r.exceptionDetails;
      // Rejections with a plain value (Tauri commands reject with strings) carry it in `value`.
      const x = d.exception;
      throw new Error(x?.description ?? (x && "value" in x ? `${d.text} ${JSON.stringify(x.value)}` : d.text));
    }
    return r.result.value;
  }

  function on(method, f) {
    if (!listeners.has(method)) listeners.set(method, []);
    listeners.get(method).push(f);
  }

  return { send, ev, on, id: target.id, close: () => ws.close() };
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
