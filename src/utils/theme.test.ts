import { describe, expect, it } from 'vitest';
import {
  isThemePreference,
  readStoredPreference,
  resolveTheme,
  THEME_KEY,
  writeStoredPreference,
} from './theme';

const mem = (o: Record<string, string>) => ({ getItem: (k: string) => o[k] ?? null });

describe('resolveTheme', () => {
  it.each([
    ['system', true, 'dark'],
    ['system', false, 'light'],
    ['light', true, 'light'],
    ['light', false, 'light'],
    ['dark', true, 'dark'],
    ['dark', false, 'dark'],
  ] as const)('%s with systemDark=%s → %s', (pref, dark, expected) => {
    expect(resolveTheme(pref, dark)).toBe(expected);
  });
});

describe('readStoredPreference', () => {
  it('returns the stored value', () =>
    expect(readStoredPreference(mem({ 'moony-theme': 'dark' }))).toBe('dark'));
  it('falls back to system on a missing key', () =>
    expect(readStoredPreference(mem({}))).toBe('system'));
  it('falls back to system on an unknown value', () =>
    expect(readStoredPreference(mem({ 'moony-theme': 'blue' }))).toBe('system'));
  it('falls back to system when the storage throws', () =>
    expect(
      readStoredPreference({
        getItem: () => {
          throw new Error('blocked');
        },
      })
    ).toBe('system'));
  it('falls back to system without a storage', () =>
    expect(readStoredPreference(undefined)).toBe('system'));
});

describe('writeStoredPreference', () => {
  it('stores the preference under the theme key', () => {
    const written: Record<string, string> = {};
    writeStoredPreference('dark', {
      setItem: (k: string, v: string) => {
        written[k] = v;
      },
    });
    expect(written).toEqual({ [THEME_KEY]: 'dark' });
  });
  it('does not throw when the storage throws', () => {
    expect(() =>
      writeStoredPreference('light', {
        setItem: () => {
          throw new Error('blocked');
        },
      })
    ).not.toThrow();
  });
});

describe('isThemePreference', () => {
  it('accepts the three values and nothing else', () => {
    expect(['system', 'light', 'dark'].every(isThemePreference)).toBe(true);
    expect(isThemePreference('auto')).toBe(false);
    expect(isThemePreference(null)).toBe(false);
  });
});
