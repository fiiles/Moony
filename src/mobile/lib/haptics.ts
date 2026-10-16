import { selectionFeedback } from '@tauri-apps/plugin-haptics';

/** A fast scrub over a long period changes the day every few pixels; ticks closer than this merge. */
const MIN_TICK_GAP_MS = 30;
let lastTick = 0;

/**
 * The light selection tick of a scrub (spec §10.4: one per day change). A no-op where the plugin
 * is not there (desktop, browser); never throws.
 */
export function selectionTick(): void {
  const now = Date.now();
  if (now - lastTick < MIN_TICK_GAP_MS) return;
  lastTick = now;
  try {
    void selectionFeedback().catch(() => undefined);
  } catch {
    // Outside Tauri the plugin's invoke is missing: nothing to tick.
  }
}
