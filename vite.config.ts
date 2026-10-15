import { defineConfig, type Plugin, type WebSocketClient } from 'vite';
import { configDefaults } from 'vitest/config';
import react from '@vitejs/plugin-react';
import path from 'path';
import { readdirSync, readFileSync, statSync } from 'fs';

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

interface FileResult {
  id: string;
  status: number;
  mime: string | null;
  bytes: number[];
}

// Dev-only relay for src/dev/browser-bridge.ts: a browser tab sends commands
// over the HMR websocket, the Tauri window executes them with the real
// backend, and the result is routed back to the tab that asked.
function moonyBrowserBridge(): Plugin {
  // A minimized Tauri window on macOS has its JavaScript suspended: relayed
  // commands would hang forever (DEV-01). The executor sends a heartbeat every
  // 2 s; when it goes quiet we fail new and pending requests with a clear
  // message instead of letting the browser tab wait indefinitely.
  const HEARTBEAT_STALE_MS = 6_000;
  const SUSPENDED_MSG =
    '[browser-bridge] Tauri window is not responding (minimized or suspended?) — bring the Moony window to the front and retry.';

  return {
    name: 'moony-browser-bridge',
    apply: 'serve',
    configureServer(server) {
      let executor: WebSocketClient | null = null;
      let lastHeartbeat = 0;
      const origin = new Map<string, WebSocketClient>();

      const executorAlive = () =>
        executor !== null && Date.now() - lastHeartbeat < HEARTBEAT_STALE_MS;

      const failPending = (message: string) => {
        for (const [id, client] of origin)
          client.send('moony:result', { id, ok: false, value: message });
        origin.clear();
      };

      server.ws.on('moony:executor-hello', (_data, client) => {
        executor = client;
        lastHeartbeat = Date.now();
        server.config.logger.info('[browser-bridge] Tauri window connected');
      });

      server.ws.on('moony:heartbeat', () => {
        lastHeartbeat = Date.now();
      });

      server.ws.on('moony:invoke', (req: { id: string; cmd: string }, client) => {
        if (!executor) {
          client.send('moony:result', {
            id: req.id,
            ok: false,
            value:
              '[browser-bridge] Tauri window not connected — run `npm run tauri dev` and keep its window open.',
          });
          return;
        }
        if (!executorAlive()) {
          client.send('moony:result', { id: req.id, ok: false, value: SUSPENDED_MSG });
          return;
        }
        origin.set(req.id, client);
        executor.send('moony:exec', req);
      });

      server.ws.on('moony:result', (res: { id: string }) => {
        origin.get(res.id)?.send('moony:result', res);
        origin.delete(res.id);
      });

      // Attachment bytes (`moony-file` URLs) are not reachable from a browser: the tab loads
      // /__moony-file/<path>, and the Tauri window fetches the real URL and sends the bytes back.
      let nextFileId = 0;
      const pendingFiles = new Map<string, (result: FileResult) => void>();
      server.middlewares.use('/__moony-file', (req, res) => {
        if (!executor || !executorAlive()) {
          res.statusCode = 503;
          res.end();
          return;
        }
        const id = String(++nextFileId);
        const timer = setTimeout(() => {
          pendingFiles.delete(id);
          res.statusCode = 504;
          res.end();
        }, 15_000);
        pendingFiles.set(id, (result) => {
          clearTimeout(timer);
          res.statusCode = result.status;
          res.setHeader('Cache-Control', 'no-store');
          if (result.mime) res.setHeader('Content-Type', result.mime);
          res.end(new Uint8Array(result.bytes));
        });
        executor.send('moony:file', {
          id,
          path: decodeURIComponent(((req as { url?: string }).url ?? '/').slice(1)),
        });
      });
      server.ws.on('moony:file-result', (result: FileResult) => {
        pendingFiles.get(result.id)?.(result);
        pendingFiles.delete(result.id);
      });

      server.ws.on('moony:subscribe', (data) => executor?.send('moony:subscribe', data));
      server.ws.on('moony:event', (data) => server.ws.send('moony:event', data));

      const watchdog = setInterval(() => {
        if (origin.size > 0 && executor && !executorAlive()) {
          server.config.logger.warn(
            `[browser-bridge] executor silent for ${Math.round((Date.now() - lastHeartbeat) / 1000)} s, failing ${origin.size} pending request(s)`
          );
          failPending(SUSPENDED_MSG);
        }
      }, 2_000);
      server.httpServer?.once('close', () => clearInterval(watchdog));
    },
  };
}

// pdf.js loads its wasm decoders (and their JS fallbacks), standard fonts, CMaps and ICC profiles
// by URL at runtime. They are served and bundled from node_modules under /pdfjs/, so the viewer
// stays on 'self' under the app CSP (plan Task 0 outcome).
function pdfjsAssets(): Plugin {
  const root = path.resolve(import.meta.dirname, 'node_modules/pdfjs-dist');
  const dirs = ['wasm', 'standard_fonts', 'cmaps', 'iccs'];
  const files = () =>
    dirs.flatMap((dir) =>
      readdirSync(path.join(root, dir))
        .map((name) => `${dir}/${name}`)
        .filter((rel) => statSync(path.join(root, rel)).isFile())
    );
  const contentType = (rel: string) =>
    rel.endsWith('.wasm')
      ? 'application/wasm'
      : /\.m?js$/.test(rel)
        ? 'text/javascript'
        : 'application/octet-stream';

  return {
    name: 'moony-pdfjs-assets',
    configureServer(server) {
      server.middlewares.use('/pdfjs', (req, res, next) => {
        const url = (req as { url?: string }).url ?? '/';
        const rel = decodeURIComponent(url.split('?')[0]).replace(/^\/+/, '');
        if (!files().includes(rel)) return next();
        res.setHeader('Content-Type', contentType(rel));
        res.end(readFileSync(path.join(root, rel)));
      });
    },
    generateBundle() {
      for (const rel of files()) {
        this.emitFile({
          type: 'asset',
          fileName: `pdfjs/${rel}`,
          source: readFileSync(path.join(root, rel)),
        });
      }
    },
  };
}

// https://vite.dev/config/
export default defineConfig(async ({ mode }) => ({
  plugins: [react(), ...(mode === 'test' ? [] : [moonyBrowserBridge(), pdfjsAssets()])],

  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, './src'),
      '@shared': path.resolve(import.meta.dirname, './shared'),
    },
  },

  build: {
    // Vite 8 defaults to lightningcss, which fails on this project's CSS
    cssMinify: 'esbuild',
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**'],
    },
  },

  test: {
    globals: true,
    environment: 'node',
    // Don't run duplicate test files from transient agent worktrees under
    // .claude or from generated coverage output
    exclude: [...configDefaults.exclude, '**/.claude/**', '**/coverage/**'],
  },
}));
