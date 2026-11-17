import { describe, expect, it } from 'vitest';
import { sheetCloseVerdict } from './sheet-close';

describe('sheetCloseVerdict', () => {
  it('closes a sheet that is neither locked nor dirty', () => {
    expect(sheetCloseVerdict({})).toBe('close');
    expect(sheetCloseVerdict({ locked: false, dirty: false })).toBe('close');
  });

  it('asks first when the sheet holds input', () => {
    expect(sheetCloseVerdict({ dirty: true })).toBe('ask');
  });

  it('keeps a locked sheet open, even with input in it', () => {
    expect(sheetCloseVerdict({ locked: true })).toBe('stay');
    expect(sheetCloseVerdict({ locked: true, dirty: true })).toBe('stay');
  });
});
