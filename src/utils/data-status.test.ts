import { describe, expect, it } from 'vitest';
import { dataStatus, type DataStatusInput } from './data-status';
import type { SyncStateView } from './sync-state';

const sync = (kind: SyncStateView['kind'], tone: SyncStateView['tone']): SyncStateView => ({
  kind,
  tone,
  labelKey: `indicator.${kind}`,
  rowKey: `phone.row.${kind}`,
  days: 0,
  ageFrom: null,
});

const calm: DataStatusInput = {
  pricesStale: false,
  sync: null,
  loadingHistory: false,
  recalculating: false,
};

describe('dataStatus', () => {
  it('is fresh when nothing is wrong or running', () => {
    expect(dataStatus(calm)).toEqual({ tone: 'fresh', label: { kind: 'fresh' } });
    expect(dataStatus({ ...calm, sync: sync('synced', 'fresh') })).toEqual({
      tone: 'fresh',
      label: { kind: 'fresh' },
    });
  });

  it('names a single problem', () => {
    expect(dataStatus({ ...calm, pricesStale: true })).toEqual({
      tone: 'stale',
      label: { kind: 'pricesStale' },
    });
    const broken = sync('unreachableLong', 'stale');
    expect(dataStatus({ ...calm, sync: broken })).toEqual({
      tone: 'stale',
      label: { kind: 'sync', sync: broken },
    });
  });

  it('counts two problems', () => {
    expect(dataStatus({ ...calm, pricesStale: true, sync: sync('error', 'stale') })).toEqual({
      tone: 'stale',
      label: { kind: 'problems', count: 2 },
    });
  });

  it('lets a problem win over the page status', () => {
    expect(
      dataStatus({ ...calm, sync: sync('paused', 'stale'), page: { text: 'Příští platba' } }).tone
    ).toBe('stale');
  });

  it('shows the page status over work in progress and the fresh state', () => {
    const page = { text: 'Příští platba 1. 12.', tone: 'neutral' as const };
    expect(
      dataStatus({ ...calm, recalculating: true, sync: sync('syncing', 'neutral'), page })
    ).toEqual({ tone: 'neutral', label: { kind: 'page', text: page.text } });
    expect(dataStatus({ ...calm, page: { text: 'Hotovo', tone: 'fresh' } }).tone).toBe('fresh');
    expect(dataStatus({ ...calm, page: { text: 'Bez tónu' } }).tone).toBe('neutral');
  });

  it('shows work in progress as neutral, history first', () => {
    expect(dataStatus({ ...calm, loadingHistory: true, recalculating: true }).label).toEqual({
      kind: 'loadingHistory',
    });
    expect(dataStatus({ ...calm, recalculating: true, sync: sync('syncing', 'neutral') })).toEqual({
      tone: 'neutral',
      label: { kind: 'recalculating' },
    });
    const running = sync('syncing', 'neutral');
    expect(dataStatus({ ...calm, sync: running })).toEqual({
      tone: 'neutral',
      label: { kind: 'sync', sync: running },
    });
  });
});
