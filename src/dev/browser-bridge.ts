/**
 * Dev-only browser bridge.
 *
 * Lets the app run in a regular browser against the REAL Rust backend while
 * `npm run tauri dev` is running. Both the Tauri window and the browser load
 * the same Vite page; this module decides its role at startup:
 *
 * - Inside Tauri (`__TAURI_INTERNALS__` exists): acts as the executor. It
 *   receives commands relayed over the Vite HMR websocket, runs them with the
 *   real `invoke()`, and sends the results back.
 * - In a plain browser: installs a `__TAURI_INTERNALS__` polyfill whose
 *   `invoke()` forwards every command to the executor.
 *
 * The relay itself lives in the `moonyBrowserBridge` plugin in vite.config.ts.
 * Never shipped: main.tsx only imports this behind `import.meta.env.DEV`.
 */

type Json = unknown;

interface ExecRequest {
  id: string;
  cmd: string;
  args?: Json;
}

interface ExecResult {
  id: string;
  ok: boolean;
  value: Json;
}

// Commands that must not be proxied from the browser (would restart or
// update the desktop app underneath you).
const BLOCKED_PREFIXES = ['plugin:updater|', 'plugin:process|'];

const hot = import.meta.hot;

/** Make args/results JSON-safe: typed arrays → number[], IPC-serializable objects → their IPC form. */
function toWire(value: Json): Json {
  if (value === null || typeof value !== 'object') return value;
  if (value instanceof ArrayBuffer) return Array.from(new Uint8Array(value));
  if (ArrayBuffer.isView(value)) {
    return Array.from(new Uint8Array(value.buffer, value.byteOffset, value.byteLength));
  }
  if (Array.isArray(value)) return value.map(toWire);
  const ipc = (value as Record<string, unknown>)['__TAURI_TO_IPC_KEY__'];
  if (typeof ipc === 'function') return toWire(ipc.call(value));
  const out: Record<string, Json> = {};
  for (const [k, v] of Object.entries(value)) out[k] = toWire(v);
  return out;
}

function startExecutor(internals: { invoke: (cmd: string, args?: Json) => Promise<Json> }) {
  if (!hot) return;
  const hello = () => hot.send('moony:executor-hello', {});
  hello();
  hot.on('vite:ws:connect', hello);
  // Heartbeat: lets the relay detect a suspended (minimized) window — see DEV-01.
  setInterval(() => hot.send('moony:heartbeat', {}), 2000);

  hot.on('moony:exec', async (req: ExecRequest) => {
    let result: ExecResult;
    try {
      result = { id: req.id, ok: true, value: toWire(await internals.invoke(req.cmd, req.args)) };
    } catch (error) {
      result = { id: req.id, ok: false, value: toWire(error) };
    }
    hot.send('moony:result', result);
  });

  // Forward backend events the browser subscribed to.
  const subscribed = new Set<string>();
  hot.on('moony:subscribe', async ({ event }: { event: string }) => {
    if (subscribed.has(event)) return;
    subscribed.add(event);
    const { listen } = await import('@tauri-apps/api/event');
    await listen(event, (e) => hot.send('moony:event', { event, payload: toWire(e.payload) }));
  });

  console.info('[browser-bridge] Tauri window is serving commands for browser tabs.');
}

function installBrowserPolyfill() {
  if (!hot) return;
  const pending = new Map<string, { resolve: (v: Json) => void; reject: (e: Json) => void }>();
  const callbacks = new Map<number, (payload: Json) => void>();
  // event name → listener ids registered in this tab
  const listeners = new Map<string, Set<number>>();
  let nextCallbackId = 1;

  hot.on('moony:result', ({ id, ok, value }: ExecResult) => {
    const p = pending.get(id);
    if (!p) return; // result for another tab
    pending.delete(id);
    if (ok) p.resolve(value);
    else p.reject(value);
  });

  hot.on('moony:event', ({ event, payload }: { event: string; payload: Json }) => {
    for (const handlerId of listeners.get(event) ?? []) {
      callbacks.get(handlerId)?.({ event, id: handlerId, payload });
    }
  });

  const relay = (cmd: string, args?: Json) =>
    new Promise<Json>((resolve, reject) => {
      const id = crypto.randomUUID();
      pending.set(id, { resolve, reject });
      hot.send('moony:invoke', { id, cmd, args: toWire(args ?? {}) });
    });

  const invoke = async (cmd: string, args?: Record<string, unknown>) => {
    if (BLOCKED_PREFIXES.some((p) => cmd.startsWith(p))) {
      throw new Error(`[browser-bridge] ${cmd} is disabled in the browser`);
    }
    // Events are handled locally; the executor subscribes on our behalf.
    if (cmd === 'plugin:event|listen') {
      const event = String(args?.event);
      const handlerId = Number(args?.handler);
      if (!listeners.has(event)) listeners.set(event, new Set());
      listeners.get(event)!.add(handlerId);
      hot.send('moony:subscribe', { event });
      return handlerId;
    }
    if (cmd === 'plugin:event|unlisten') {
      listeners.get(String(args?.event))?.delete(Number(args?.eventId));
      return null;
    }
    return relay(cmd, args);
  };

  const w = window as unknown as Record<string, unknown>;
  w.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback: (cb: (payload: Json) => void, once = false) => {
      const id = nextCallbackId++;
      callbacks.set(id, (payload) => {
        if (once) callbacks.delete(id);
        cb(payload);
      });
      return id;
    },
    unregisterCallback: (id: number) => callbacks.delete(id),
    // Asset protocol isn't reachable from a browser; images from disk won't load.
    convertFileSrc: (path: string, protocol = 'asset') =>
      `http://${protocol}.localhost/${encodeURIComponent(path)}`,
    metadata: {
      currentWindow: { label: 'main' },
      currentWebview: { windowLabel: 'main', label: 'main' },
    },
  };
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: (event: string, id: number) => listeners.get(event)?.delete(id),
  };

  console.info(
    '[browser-bridge] Running in browser mode — commands are relayed to the Tauri window (keep `npm run tauri dev` running).'
  );
}

const existing = (
  window as unknown as {
    __TAURI_INTERNALS__?: { invoke: (cmd: string, args?: Json) => Promise<Json> };
  }
).__TAURI_INTERNALS__;
if (existing) startExecutor(existing);
else installBrowserPolyfill();
