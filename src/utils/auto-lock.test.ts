import { describe, expect, it } from 'vitest';
import {
  ACTIVITY_THROTTLE_MS,
  AUTO_LOCK_OPTIONS,
  AUTO_LOCK_STORAGE_KEY,
  DEFAULT_AUTO_LOCK_MINUTES,
  autoLockDeadline,
  isAutoLockDue,
  msUntilAutoLock,
  parseAutoLockMinutes,
  readAutoLockMinutes,
  shouldRecordActivity,
  writeAutoLockMinutes,
} from './auto-lock';

const MINUTE = 60_000;

function fakeStorage(initial?: string) {
  const data = new Map<string, string>();
  if (initial !== undefined) data.set(AUTO_LOCK_STORAGE_KEY, initial);
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
  };
}

describe('auto-lock setting', () => {
  it('offers off / 5 / 15 / 30 / 60 minutes and defaults to 15', () => {
    expect([...AUTO_LOCK_OPTIONS]).toEqual([0, 5, 15, 30, 60]);
    expect(DEFAULT_AUTO_LOCK_MINUTES).toBe(15);
  });

  it('parses every offered value, including off', () => {
    for (const option of AUTO_LOCK_OPTIONS) {
      expect(parseAutoLockMinutes(String(option))).toBe(option);
    }
  });

  it('falls back to the default for missing or unknown values', () => {
    for (const raw of [null, undefined, '', '  ', 'abc', '7', '-5', '15.5', 'NaN']) {
      expect(parseAutoLockMinutes(raw)).toBe(15);
    }
  });

  it('reads and writes under the documented localStorage key', () => {
    const storage = fakeStorage();
    expect(readAutoLockMinutes(storage)).toBe(15);
    writeAutoLockMinutes(30, storage);
    expect(storage.data.get('moony-auto-lock-minutes')).toBe('30');
    expect(readAutoLockMinutes(storage)).toBe(30);
    writeAutoLockMinutes(0, storage);
    expect(readAutoLockMinutes(storage)).toBe(0);
  });

  it('survives storage that throws', () => {
    const broken = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(readAutoLockMinutes(broken)).toBe(15);
    expect(() => writeAutoLockMinutes(5, broken)).not.toThrow();
  });
});

describe('auto-lock deadline', () => {
  it('is last activity plus the configured minutes, or null when off', () => {
    expect(autoLockDeadline(1_000, 5)).toBe(1_000 + 5 * MINUTE);
    expect(autoLockDeadline(1_000, 60)).toBe(1_000 + 60 * MINUTE);
    expect(autoLockDeadline(1_000, 0)).toBeNull();
  });

  it('is not due before the limit and due exactly at it', () => {
    const last = 10_000;
    expect(isAutoLockDue(last, last, 5)).toBe(false);
    expect(isAutoLockDue(last + 5 * MINUTE - 1, last, 5)).toBe(false);
    expect(isAutoLockDue(last + 5 * MINUTE, last, 5)).toBe(true);
    expect(isAutoLockDue(last + 5 * MINUTE + 1, last, 5)).toBe(true);
  });

  it('is due after the clock jumped far ahead, as after sleep', () => {
    const last = 0;
    const eightHoursLater = 8 * 60 * MINUTE;
    expect(isAutoLockDue(eightHoursLater, last, 60)).toBe(true);
    expect(isAutoLockDue(eightHoursLater, last, 15)).toBe(true);
  });

  it('is never due while off', () => {
    expect(isAutoLockDue(Number.MAX_SAFE_INTEGER, 0, 0)).toBe(false);
  });

  it('is not due when the clock moved backwards', () => {
    expect(isAutoLockDue(500, 10_000, 5)).toBe(false);
  });

  it('reports the remaining wait, clamped to 0..interval', () => {
    const last = 100_000;
    expect(msUntilAutoLock(last, last, 5)).toBe(5 * MINUTE);
    expect(msUntilAutoLock(last + 2 * MINUTE, last, 5)).toBe(3 * MINUTE);
    expect(msUntilAutoLock(last + 5 * MINUTE, last, 5)).toBe(0);
    expect(msUntilAutoLock(last + 9 * MINUTE, last, 5)).toBe(0);
    // Clock moved backwards: never wait longer than one interval.
    expect(msUntilAutoLock(last - 3 * MINUTE, last, 5)).toBe(5 * MINUTE);
    expect(msUntilAutoLock(last, last, 0)).toBeNull();
  });
});

describe('activity throttle', () => {
  it('records at most once per throttle window', () => {
    expect(shouldRecordActivity(1_000, 1_000)).toBe(false);
    expect(shouldRecordActivity(1_000 + ACTIVITY_THROTTLE_MS - 1, 1_000)).toBe(false);
    expect(shouldRecordActivity(1_000 + ACTIVITY_THROTTLE_MS, 1_000)).toBe(true);
    expect(shouldRecordActivity(5_000, 1_000)).toBe(true);
  });

  it('records again after a backwards clock jump', () => {
    expect(shouldRecordActivity(500, 10_000)).toBe(true);
  });

  it('accepts a custom window', () => {
    expect(shouldRecordActivity(1_200, 1_000, 100)).toBe(true);
    expect(shouldRecordActivity(1_050, 1_000, 100)).toBe(false);
  });
});
