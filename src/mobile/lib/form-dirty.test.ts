import { describe, expect, it } from 'vitest';
import { formChanged } from './form-dirty';

describe('formChanged', () => {
  it('is false for the form it opened with', () => {
    const start = { amount: '', date: '2026-11-07', negative: false, tags: ['a'] };
    expect(formChanged(start, { ...start, tags: ['a'] })).toBe(false);
  });

  it('is true once a value differs', () => {
    expect(formChanged({ amount: '' }, { amount: '12,5' })).toBe(true);
    expect(formChanged({ negative: false }, { negative: true })).toBe(true);
    expect(formChanged({ tags: ['a'] }, { tags: ['a', 'b'] })).toBe(true);
    expect(formChanged({ nested: { a: 1 } }, { nested: { a: 2 } })).toBe(true);
  });

  it('takes whitespace for nothing typed', () => {
    expect(formChanged({ name: '' }, { name: '   ' })).toBe(false);
    expect(formChanged({ name: 'Byt' }, { name: 'Byt ' })).toBe(true);
  });

  it('takes a field typed into and cleared again for nothing typed', () => {
    expect(formChanged({}, { a: '' })).toBe(false);
    expect(formChanged({}, { note: undefined })).toBe(false);
    expect(formChanged({ choice: null }, { choice: '' })).toBe(false);
    expect(formChanged({}, { a: '12' })).toBe(true);
  });
});
