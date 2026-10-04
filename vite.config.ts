import { defineConfig, type Plugin, type WebSocketClient } from 'vite';
import { configDefaults } from 'vitest/config';
import react from '@vitejs/plugin-react';
import path from 'path';

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

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

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), moonyBrowserBridge()],

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
