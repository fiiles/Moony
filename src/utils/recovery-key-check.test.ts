import { describe, expect, it } from 'vitest';
import {
  normalizeGroupInput,
  pickVerificationGroups,
  recoveryKeyFileContents,
  recoveryKeyGroups,
  verifyGroups,
} from './recovery-key-check';

const KEY = 'ABCD-EFGH-JKLM-NPQR-STUV-WXYZ';

describe('recoveryKeyGroups', () => {
  it('splits on dashes and whitespace and uppercases', () => {
    expect(recoveryKeyGroups(KEY)).toEqual(['ABCD', 'EFGH', 'JKLM', 'NPQR', 'STUV', 'WXYZ']);
    expect(recoveryKeyGroups('abcd efgh')).toEqual(['ABCD', 'EFGH']);
  });
});

describe('pickVerificationGroups', () => {
  it('returns two distinct ascending indexes within range', () => {
    for (let i = 0; i < 200; i++) {
      const [a, b] = pickVerificationGroups(6);
      expect(a).toBeGreaterThanOrEqual(0);
      expect(b).toBeLessThan(6);
      expect(a).toBeLessThan(b);
    }
  });

  it('is deterministic for a given random source', () => {
    const seq = [0.99, 0.0];
    const random = () => seq.shift() ?? 0;
    expect(pickVerificationGroups(6, random)).toEqual([0, 5]);
  });
});

describe('verifyGroups', () => {
  it('accepts matching groups regardless of case and spacing', () => {
    expect(verifyGroups(KEY, [1, 4], ['efgh', ' stuv '])).toBe(true);
    expect(verifyGroups(KEY, [1, 4], ['EFGH', 'STUV'])).toBe(true);
  });

  it('rejects wrong or swapped groups', () => {
    expect(verifyGroups(KEY, [1, 4], ['STUV', 'EFGH'])).toBe(false);
    expect(verifyGroups(KEY, [1, 4], ['EFGX', 'STUV'])).toBe(false);
    expect(verifyGroups(KEY, [1, 9], ['EFGH', 'STUV'])).toBe(false);
  });

  it('normalizes typed input', () => {
    expect(normalizeGroupInput(' ab-cd ')).toBe('ABCD');
  });
});

describe('recoveryKeyFileContents', () => {
  it('contains the key and the note', () => {
    const text = recoveryKeyFileContents(KEY, { title: 'Moony recovery key', note: 'Keep safe.' });
    expect(text).toContain(KEY);
    expect(text.startsWith('Moony recovery key')).toBe(true);
    expect(text).toContain('Keep safe.');
  });
});
