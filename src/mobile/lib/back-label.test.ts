import { describe, expect, it } from 'vitest';
import { backLabel, backState } from './back-label';

describe('backState', () => {
  it('carries the label of the screen the push leaves', () => {
    expect(backLabel(backState('Byt Lipová'), 'Finance')).toBe('Byt Lipová');
  });

  it('is no state without a label, so the target keeps its own', () => {
    expect(backState(undefined)).toBeUndefined();
    expect(backState('')).toBeUndefined();
    expect(backState('   ')).toBeUndefined();
  });
});

describe('backLabel', () => {
  it('falls back to the screen’s own label without state (a deep link, a cold start)', () => {
    expect(backLabel(null, 'Finance')).toBe('Finance');
    expect(backLabel(undefined, 'Finance')).toBe('Finance');
  });

  it('falls back when the state carries no usable label', () => {
    expect(backLabel({}, 'Majetek')).toBe('Majetek');
    expect(backLabel({ moonyBack: '' }, 'Majetek')).toBe('Majetek');
    expect(backLabel({ moonyBack: '  ' }, 'Majetek')).toBe('Majetek');
    expect(backLabel({ moonyBack: 42 }, 'Majetek')).toBe('Majetek');
    expect(backLabel('Přehled', 'Majetek')).toBe('Majetek');
  });

  it('keeps the label under a sheet’s entry, which spreads the state it found', () => {
    const state = { ...backState('Přehled'), moonySheet: '3' };
    expect(backLabel(state, 'Finance')).toBe('Přehled');
  });
});
