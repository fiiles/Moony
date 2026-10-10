import { describe, expect, it } from 'vitest';
import {
  PLAN_EXPANSION_STORAGE_KEY,
  allExpanded,
  readExpanded,
  setKeys,
  toggleKey,
  writeExpanded,
} from './plan-expansion';

function memoryStorage(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
    data,
  };
}

const throwing = {
  getItem: () => {
    throw new Error('blocked');
  },
  setItem: () => {
    throw new Error('blocked');
  },
};

describe('plan-expansion', () => {
  it('reads an empty set when nothing is stored, the value is malformed or storage is unavailable', () => {
    expect(readExpanded(memoryStorage()).size).toBe(0);
    expect(readExpanded(memoryStorage({ [PLAN_EXPANSION_STORAGE_KEY]: '{bad' })).size).toBe(0);
    expect(readExpanded(memoryStorage({ [PLAN_EXPANSION_STORAGE_KEY]: '"x"' })).size).toBe(0);
    expect(readExpanded(memoryStorage({ [PLAN_EXPANSION_STORAGE_KEY]: '[1,"a"]' }))).toEqual(
      new Set(['a'])
    );
    expect(readExpanded(throwing).size).toBe(0);
    expect(readExpanded(null).size).toBe(0);
  });

  it('round-trips the expanded keys', () => {
    const storage = memoryStorage();
    writeExpanded(storage, new Set(['stockDividends', 'personalLoans']));
    expect([...readExpanded(storage)].sort()).toEqual(['personalLoans', 'stockDividends']);
    expect(() => writeExpanded(throwing, new Set(['a']))).not.toThrow();
    expect(() => writeExpanded(null, new Set(['a']))).not.toThrow();
  });

  it('toggles one key without mutating the input', () => {
    const before = new Set(['a']);
    const after = toggleKey(before, 'b');
    expect([...after].sort()).toEqual(['a', 'b']);
    expect([...toggleKey(after, 'a')]).toEqual(['b']);
    expect([...before]).toEqual(['a']);
  });

  it('expands or collapses a section without touching other sections', () => {
    const keys = new Set(['other']);
    const opened = setKeys(keys, ['a', 'b'], true);
    expect([...opened].sort()).toEqual(['a', 'b', 'other']);
    const closed = setKeys(opened, ['a', 'b'], false);
    expect([...closed]).toEqual(['other']);
    expect([...keys]).toEqual(['other']);
  });

  it('reports whether every category of a section is expanded', () => {
    expect(allExpanded(new Set(['a', 'b']), ['a', 'b'])).toBe(true);
    expect(allExpanded(new Set(['a']), ['a', 'b'])).toBe(false);
    expect(allExpanded(new Set(), [])).toBe(false);
  });
});
