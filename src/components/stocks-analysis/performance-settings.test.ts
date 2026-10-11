import { describe, expect, it } from 'vitest';
import { DAY, dayFromIso } from '@/utils/stock-performance';
import {
  CUSTOM_KEY,
  GROUP_KEY,
  PERIOD_KEY,
  loadGroupKey,
  loadPeriod,
  parseStoredRange,
  savePeriod,
  saveGroupKey,
} from './performance-settings';

/** In-memory stand-in for `localStorage`. */
function memory(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
  };
}

const d = (iso: string) => dayFromIso(iso) as number;

describe('parseStoredRange', () => {
  it('reads two ISO days', () => {
    expect(parseStoredRange('2026-01-05|2026-09-30')).toEqual({
      from: d('2026-01-05'),
      to: d('2026-09-30'),
    });
  });

  it('rejects anything else', () => {
    expect(parseStoredRange(null)).toBeNull();
    expect(parseStoredRange('')).toBeNull();
    expect(parseStoredRange('2026-01-05')).toBeNull();
    expect(parseStoredRange('2026-02-30|2026-09-30')).toBeNull();
    expect(parseStoredRange('2026-09-30|2026-01-05')).toBeNull();
    expect(parseStoredRange('2026-09-29|2026-09-30')).toBeNull();
  });

  it('accepts a range of exactly two days', () => {
    const from = d('2026-09-28');
    expect(parseStoredRange('2026-09-28|2026-09-30')).toEqual({ from, to: from + 2 * DAY });
  });
});

describe('loadPeriod', () => {
  it('defaults to this year', () => {
    expect(loadPeriod(memory())).toEqual({ period: 'ytd', custom: null });
  });

  it('restores a preset and a stored custom range', () => {
    const storage = memory({ [PERIOD_KEY]: '3y', [CUSTOM_KEY]: '2026-01-05|2026-09-30' });
    expect(loadPeriod(storage)).toEqual({
      period: '3y',
      custom: { from: d('2026-01-05'), to: d('2026-09-30') },
    });
  });

  it('restores "custom" only with a usable range', () => {
    const ok = memory({ [PERIOD_KEY]: 'custom', [CUSTOM_KEY]: '2026-01-05|2026-09-30' });
    expect(loadPeriod(ok).period).toBe('custom');
    const broken = memory({ [PERIOD_KEY]: 'custom', [CUSTOM_KEY]: 'nonsense' });
    expect(loadPeriod(broken)).toEqual({ period: 'ytd', custom: null });
  });

  it('ignores an unknown period', () => {
    expect(loadPeriod(memory({ [PERIOD_KEY]: '5y' })).period).toBe('ytd');
  });
});

describe('savePeriod', () => {
  it('writes the period and the custom range', () => {
    const storage = memory();
    savePeriod(
      { period: 'custom', custom: { from: d('2026-01-05'), to: d('2026-09-30') } },
      storage
    );
    expect(storage.data.get(PERIOD_KEY)).toBe('custom');
    expect(storage.data.get(CUSTOM_KEY)).toBe('2026-01-05|2026-09-30');
  });

  it('keeps the stored custom range when switching to a preset', () => {
    const storage = memory({ [CUSTOM_KEY]: '2026-01-05|2026-09-30' });
    savePeriod({ period: '1y', custom: null }, storage);
    expect(storage.data.get(PERIOD_KEY)).toBe('1y');
    expect(storage.data.get(CUSTOM_KEY)).toBe('2026-01-05|2026-09-30');
  });
});

describe('group key', () => {
  it('round-trips', () => {
    const storage = memory();
    expect(loadGroupKey(storage)).toBeNull();
    saveGroupKey('sector', storage);
    expect(storage.data.get(GROUP_KEY)).toBe('sector');
    expect(loadGroupKey(storage)).toBe('sector');
  });
});
