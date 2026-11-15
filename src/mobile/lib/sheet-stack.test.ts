import { describe, expect, it, vi } from 'vitest';
import { createSheetStack } from './sheet-stack';

describe('createSheetStack', () => {
  it('covers a sheet once another opens over it, and uncovers it when that one closes', () => {
    const stack = createSheetStack();
    stack.open('transaction');
    expect(stack.covered('transaction')).toBe(false);
    stack.open('picker');
    expect(stack.covered('transaction')).toBe(true);
    expect(stack.covered('picker')).toBe(false);
    stack.close('picker');
    expect(stack.covered('transaction')).toBe(false);
  });

  it('keeps a sheet in place when it is opened twice, and ignores unknown ids', () => {
    const stack = createSheetStack();
    stack.open('a');
    stack.open('b');
    stack.open('a');
    expect(stack.covered('a')).toBe(true);
    stack.close('nope');
    expect(stack.covered('unknown')).toBe(false);
  });

  it('tells subscribers about every change and bumps its version', () => {
    const stack = createSheetStack();
    const listener = vi.fn();
    const unsubscribe = stack.subscribe(listener);
    const before = stack.version();
    stack.open('a');
    stack.open('a');
    stack.close('a');
    expect(listener).toHaveBeenCalledTimes(2);
    expect(stack.version()).toBe(before + 2);
    unsubscribe();
    stack.open('b');
    expect(listener).toHaveBeenCalledTimes(2);
  });
});
