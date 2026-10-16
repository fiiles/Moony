/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Set by the Tauri CLI for the frontend build: `ios`, `android`, `androideabi`, `darwin`, `windows`, `linux`. */
  readonly TAURI_ENV_PLATFORM?: string;
  /** Forces a shell regardless of platform: `mobile` or `desktop` (see `src/lib/platform.ts`). */
  readonly VITE_MOONY_SHELL?: string;
}
