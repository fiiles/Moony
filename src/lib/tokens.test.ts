import { afterEach, describe, expect, it, vi } from 'vitest';
import { resetTokenCache, token } from './tokens';

describe('token cache', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    resetTokenCache();
  });

  it('resetTokenCache makes token() re-read the stylesheet', () => {
    let value = '#111111';
    vi.stubGlobal('document', { documentElement: {} });
    vi.stubGlobal('getComputedStyle', () => ({ getPropertyValue: () => value }));
    expect(token('chart-line')).toBe('#111111');
    value = '#222222';
    expect(token('chart-line')).toBe('#111111'); // cached
    resetTokenCache();
    expect(token('chart-line')).toBe('#222222');
  });
});
