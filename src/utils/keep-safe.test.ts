import { describe, expect, it } from 'vitest';
import { keepSafeItems } from '@/utils/keep-safe';

describe('keepSafeItems', () => {
  it('lists nothing without the sync flag', () => {
    // Review Focus 1: Security reads as today without the flag.
    expect(keepSafeItems(false, true)).toEqual([]);
    expect(keepSafeItems(false, false)).toEqual([]);
  });

  it('lists the device secrets, and the Kit while sync is on', () => {
    expect(keepSafeItems(true, false)).toEqual(['password', 'recoveryKey']);
    expect(keepSafeItems(true, true)).toEqual(['password', 'recoveryKey', 'kit']);
  });
});
