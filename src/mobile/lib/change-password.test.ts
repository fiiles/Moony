import { describe, expect, it } from 'vitest';
import { canConfirmChange } from './change-password';

describe('canConfirmChange', () => {
  it('waits for the two retyped groups', () => {
    expect(canConfirmChange({ verified: false, confirming: false })).toBe(false);
  });

  it('takes one tap only: not again while the change is running', () => {
    expect(canConfirmChange({ verified: true, confirming: true })).toBe(false);
    expect(canConfirmChange({ verified: false, confirming: true })).toBe(false);
  });

  it('changes the password once the key is verified', () => {
    expect(canConfirmChange({ verified: true, confirming: false })).toBe(true);
  });
});
