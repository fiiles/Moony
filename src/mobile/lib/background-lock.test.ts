import { describe, expect, it } from 'vitest';
import {
  BACKGROUND_LOCK_KEY,
  DEFAULT_BACKGROUND_LOCK_MINUTES,
  HIDDEN_AT_KEY,
  clearHiddenAt,
  readBackgroundLockMinutes,
  readHiddenAt,
  rememberHiddenAt,
  shouldLockOnResume,
} from './background-lock';

/** A Map behind the Storage methods the helpers use. */
function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
    removeItem: (key: string) => void values.delete(key),
  };
}

const throwing = {
  getItem: (): string | null => {
    throw new Error('blocked');
  },
  setItem: (): void => {
    throw new Error('blocked');
  },
  removeItem: (): void => {
    throw new Error('blocked');
  },
};

const MINUTE = 60_000;

describe('shouldLockOnResume', () => {
  it('never locks when the app was not hidden', () => {
    for (const minutes of [0, 1, 5, 15] as const) {
      expect(shouldLockOnResume(null, 10 * 60 * MINUTE, minutes)).toBe(false);
    }
  });

  it('"Ihned" (0) locks on every return from the background', () => {
    expect(shouldLockOnResume(1_000, 1_000, 0)).toBe(true);
    expect(shouldLockOnResume(1_000, 1_001, 0)).toBe(true);
  });

  it('locks exactly at the threshold, not a millisecond before', () => {
    const hiddenAt = 1_000_000;
    expect(shouldLockOnResume(hiddenAt, hiddenAt + MINUTE - 1, 1)).toBe(false);
    expect(shouldLockOnResume(hiddenAt, hiddenAt + MINUTE, 1)).toBe(true);
    expect(shouldLockOnResume(hiddenAt, hiddenAt + MINUTE + 1, 1)).toBe(true);
    expect(shouldLockOnResume(hiddenAt, hiddenAt + 5 * MINUTE - 1, 5)).toBe(false);
    expect(shouldLockOnResume(hiddenAt, hiddenAt + 5 * MINUTE, 5)).toBe(true);
    expect(shouldLockOnResume(hiddenAt, hiddenAt + 15 * MINUTE - 1, 15)).toBe(false);
    expect(shouldLockOnResume(hiddenAt, hiddenAt + 15 * MINUTE, 15)).toBe(true);
  });

  it('stays unlocked for a short visit to another app', () => {
    expect(shouldLockOnResume(1_000_000, 1_000_000 + 20_000, 1)).toBe(false);
  });

  it('does not lock when the clock went backwards, except for "Ihned"', () => {
    expect(shouldLockOnResume(2_000_000, 1_000_000, 1)).toBe(false);
    expect(shouldLockOnResume(2_000_000, 1_000_000, 0)).toBe(true);
  });
});

describe('readBackgroundLockMinutes', () => {
  it('defaults to one minute', () => {
    expect(DEFAULT_BACKGROUND_LOCK_MINUTES).toBe(1);
    expect(readBackgroundLockMinutes(memoryStorage())).toBe(1);
  });

  it('reads the four choices', () => {
    for (const minutes of [0, 1, 5, 15]) {
      const storage = memoryStorage({ [BACKGROUND_LOCK_KEY]: String(minutes) });
      expect(readBackgroundLockMinutes(storage)).toBe(minutes);
    }
  });

  it('falls back to the default for anything else', () => {
    for (const raw of ['', ' ', '2', '30', '-1', 'abc', '1.5', 'NaN', 'Infinity']) {
      expect(readBackgroundLockMinutes(memoryStorage({ [BACKGROUND_LOCK_KEY]: raw }))).toBe(1);
    }
  });

  it('falls back to the default without usable storage', () => {
    expect(readBackgroundLockMinutes(null)).toBe(1);
    expect(readBackgroundLockMinutes(undefined)).toBe(1);
    expect(readBackgroundLockMinutes(throwing)).toBe(1);
  });
});

describe('hidden-at stamp', () => {
  it('survives in the session storage until cleared', () => {
    const storage = memoryStorage();
    expect(readHiddenAt(storage)).toBeNull();
    rememberHiddenAt(storage, 1_234_567);
    expect(storage.getItem(HIDDEN_AT_KEY)).toBe('1234567');
    expect(readHiddenAt(storage)).toBe(1_234_567);
    clearHiddenAt(storage);
    expect(readHiddenAt(storage)).toBeNull();
  });

  it('reads an unreadable stamp as none', () => {
    for (const raw of ['', 'abc', 'NaN', 'Infinity', '-5']) {
      expect(readHiddenAt(memoryStorage({ [HIDDEN_AT_KEY]: raw }))).toBeNull();
    }
  });

  it('copes with storage that is missing or blocked', () => {
    expect(readHiddenAt(null)).toBeNull();
    expect(readHiddenAt(throwing)).toBeNull();
    expect(() => rememberHiddenAt(null, 1)).not.toThrow();
    expect(() => rememberHiddenAt(throwing, 1)).not.toThrow();
    expect(() => clearHiddenAt(null)).not.toThrow();
    expect(() => clearHiddenAt(throwing)).not.toThrow();
  });
});
