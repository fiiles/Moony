import { describe, expect, it } from 'vitest';
import {
  CLIPBOARD_CLEAR_MS,
  KIT_VIEW_MS,
  kitExpired,
  shouldClearClipboard,
} from './recovery-kit-lifetime';

describe('kitExpired', () => {
  it('keeps the kit until the five minutes are up', () => {
    expect(kitExpired(1000, 1000 + KIT_VIEW_MS - 1, false)).toBe(false);
  });

  it('drops the kit at the five minutes', () => {
    expect(kitExpired(1000, 1000 + KIT_VIEW_MS, false)).toBe(true);
    expect(kitExpired(1000, 1000 + KIT_VIEW_MS + 1, false)).toBe(true);
  });

  it('drops the kit as soon as the app is hidden', () => {
    expect(kitExpired(1000, 1001, true)).toBe(true);
  });

  it('has the documented lengths', () => {
    expect(KIT_VIEW_MS).toBe(300_000);
    expect(CLIPBOARD_CLEAR_MS).toBe(60_000);
  });
});

describe('shouldClearClipboard', () => {
  it('clears only while the clipboard still holds exactly the code', () => {
    expect(shouldClearClipboard('ABC', 'ABC')).toBe(true);
  });

  it('leaves anything else alone', () => {
    expect(shouldClearClipboard('', 'ABC')).toBe(false);
    expect(shouldClearClipboard(null, 'ABC')).toBe(false);
    expect(shouldClearClipboard('abc', 'ABC')).toBe(false);
    expect(shouldClearClipboard('ABC ', 'ABC')).toBe(false);
  });
});
