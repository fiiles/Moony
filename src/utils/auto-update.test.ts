import { describe, expect, it } from 'vitest';
import {
  AUTO_UPDATE_CHECK_STORAGE_KEY,
  parseAutoUpdateCheck,
  readAutoUpdateCheck,
  writeAutoUpdateCheck,
} from './auto-update';

function fakeStorage(initial?: string) {
  const data = new Map<string, string>();
  if (initial !== undefined) data.set(AUTO_UPDATE_CHECK_STORAGE_KEY, initial);
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
  };
}

describe('auto update check setting', () => {
  it('is on by default and for unreadable values', () => {
    expect(parseAutoUpdateCheck(null)).toBe(true);
    expect(parseAutoUpdateCheck(undefined)).toBe(true);
    expect(parseAutoUpdateCheck('')).toBe(true);
    expect(parseAutoUpdateCheck('yes')).toBe(true);
    expect(readAutoUpdateCheck(fakeStorage())).toBe(true);
  });

  it('is off only when stored as "false"', () => {
    expect(parseAutoUpdateCheck('false')).toBe(false);
    expect(readAutoUpdateCheck(fakeStorage('false'))).toBe(false);
  });

  it('round-trips through storage under the documented key', () => {
    const storage = fakeStorage();
    writeAutoUpdateCheck(false, storage);
    expect(storage.data.get('moony-auto-update-check')).toBe('false');
    expect(readAutoUpdateCheck(storage)).toBe(false);
    writeAutoUpdateCheck(true, storage);
    expect(readAutoUpdateCheck(storage)).toBe(true);
  });

  it('falls back to the default when storage throws', () => {
    const broken = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(readAutoUpdateCheck(broken)).toBe(true);
    expect(() => writeAutoUpdateCheck(false, broken)).not.toThrow();
  });
});
