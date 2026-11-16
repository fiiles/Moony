import type { SyncStateView } from '@/utils/sync-state';

export type DataStatusTone = 'neutral' | 'fresh' | 'stale';

export interface DataStatusInput {
  /** Any price or exchange-rate source is stale, or a held currency has no rate. */
  pricesStale: boolean;
  /** `syncState()`; `null` while sync is off. */
  sync: SyncStateView | null;
  /** What the page publishes through `useShellPage` ("Příští platba 1. 12."). */
  page?: { text: string; tone?: DataStatusTone };
  /** Historical prices are being fetched. */
  loadingHistory: boolean;
  /** A ticker's value history is being rebuilt. */
  recalculating: boolean;
}

export type DataStatusLabel =
  | { kind: 'problems'; count: number }
  | { kind: 'pricesStale' }
  | { kind: 'sync'; sync: SyncStateView }
  | { kind: 'page'; text: string }
  | { kind: 'loadingHistory' }
  | { kind: 'recalculating' }
  | { kind: 'fresh' };

/**
 * The one top-bar indicator for prices, sync and history (spec 2026-11-16 top bar). Precedence:
 * problems (named when one, counted when more), then the page's own status, then work in progress,
 * then fresh. A problem therefore shows on every page; calm states never hide the page's status.
 */
export function dataStatus(input: DataStatusInput): {
  tone: DataStatusTone;
  label: DataStatusLabel;
} {
  const problems: DataStatusLabel[] = [];
  if (input.pricesStale) problems.push({ kind: 'pricesStale' });
  if (input.sync?.tone === 'stale') problems.push({ kind: 'sync', sync: input.sync });
  if (problems.length > 1) {
    return { tone: 'stale', label: { kind: 'problems', count: problems.length } };
  }
  if (problems.length === 1) return { tone: 'stale', label: problems[0] };

  if (input.page) {
    return { tone: input.page.tone ?? 'neutral', label: { kind: 'page', text: input.page.text } };
  }
  if (input.loadingHistory) return { tone: 'neutral', label: { kind: 'loadingHistory' } };
  if (input.recalculating) return { tone: 'neutral', label: { kind: 'recalculating' } };
  if (input.sync && input.sync.tone !== 'fresh') {
    return { tone: 'neutral', label: { kind: 'sync', sync: input.sync } };
  }
  return { tone: 'fresh', label: { kind: 'fresh' } };
}
