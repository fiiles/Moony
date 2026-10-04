import type { HistoryRecalculationEvent } from '@shared/schema';

/** Name of the Tauri event the backend emits while it rebuilds a stock ticker's history. */
export const HISTORY_RECALCULATION_EVENT = 'history-recalculation';

/**
 * The tickers whose history is being rebuilt after `event`: a ticker joins the list on `running`
 * and leaves it on `done` or `failed`. Returns the same array when nothing changes, so a state
 * setter built on it skips the re-render.
 */
export function applyRecalculationEvent(
  running: readonly string[],
  event: HistoryRecalculationEvent
): readonly string[] {
  const isRunning = running.includes(event.ticker);
  if (event.status === 'running') {
    return isRunning ? running : [...running, event.ticker];
  }
  return isRunning ? running.filter((ticker) => ticker !== event.ticker) : running;
}
