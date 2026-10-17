import { describe, expect, it } from 'vitest';
import { createViewMemory } from './view-memory';

describe('createViewMemory', () => {
  it('returns the initial value until something is set', () => {
    const memory = createViewMemory();
    expect(memory.get('finance.sort', 'balance')).toBe('balance');
    expect(memory.get('finance.sort', 'name')).toBe('name');
  });

  it('returns the set value afterwards, whatever the initial value asked for', () => {
    const memory = createViewMemory();
    memory.set('finance.sort', 'name');
    expect(memory.get('finance.sort', 'balance')).toBe('name');
  });

  it('keeps the last value set', () => {
    const memory = createViewMemory();
    memory.set('finance.period', '1Y');
    memory.set('finance.period', '30D');
    expect(memory.get('finance.period', '7D')).toBe('30D');
  });

  it('keeps the keys apart', () => {
    const memory = createViewMemory();
    memory.set('finance.showMatured', true);
    expect(memory.get('finance.showMatured', false)).toBe(true);
    expect(memory.get('finance.showEnded', false)).toBe(false);
  });

  it('remembers a value that is falsy', () => {
    const memory = createViewMemory();
    memory.set('finance.showMatured', false);
    memory.set('count', 0);
    memory.set('text', '');
    expect(memory.get('finance.showMatured', true)).toBe(false);
    expect(memory.get('count', 5)).toBe(0);
    expect(memory.get('text', 'x')).toBe('');
  });

  it('is not shared between two memories', () => {
    const first = createViewMemory();
    const second = createViewMemory();
    first.set('finance.sort', 'name');
    expect(second.get('finance.sort', 'balance')).toBe('balance');
  });
});
