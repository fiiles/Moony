import { afterEach, describe, expect, it } from 'vitest';
import { clearQuietLock, requestQuietLock, takeQuietLock } from './quiet-lock';

describe('quiet lock', () => {
  afterEach(() => clearQuietLock());

  it('is off until a manual lock asks for it', () => {
    expect(takeQuietLock()).toBe(false);
  });

  it('answers true once after a manual lock, then false', () => {
    requestQuietLock();
    expect(takeQuietLock()).toBe(true);
    expect(takeQuietLock()).toBe(false);
  });

  it('is dropped when the manual lock failed', () => {
    requestQuietLock();
    clearQuietLock();
    expect(takeQuietLock()).toBe(false);
  });
});
