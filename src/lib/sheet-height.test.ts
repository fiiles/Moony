import { describe, expect, it } from 'vitest';
import { sheetHeightClass } from './sheet-height';

describe('sheetHeightClass', () => {
  it('caps a content-sized sheet at 93 % and below the top safe area', () => {
    expect(sheetHeightClass({})).toBe('max-h-[min(93%,calc(100%_-_var(--safe-top)_-_10px))]');
  });

  it('caps a full sheet at 95 % and below the top safe area', () => {
    expect(sheetHeightClass({ full: true })).toBe(
      'max-h-[min(95%,calc(100%_-_var(--safe-top)_-_10px))]'
    );
  });

  it('gives a filled sheet a fixed height with the same caps', () => {
    expect(sheetHeightClass({ fill: true })).toBe(
      'h-[min(93%,calc(100%_-_var(--safe-top)_-_10px))]'
    );
    expect(sheetHeightClass({ fill: true, full: true })).toBe(
      'h-[min(95%,calc(100%_-_var(--safe-top)_-_10px))]'
    );
  });

  it('never lets a sheet reach into the status bar or the Dynamic Island', () => {
    for (const fill of [false, true]) {
      for (const full of [false, true]) {
        expect(sheetHeightClass({ fill, full })).toContain('var(--safe-top)');
      }
    }
  });
});
