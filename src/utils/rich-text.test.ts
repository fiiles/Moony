import { describe, expect, it } from 'vitest';
import { slot, splitSlots } from './rich-text';

describe('splitSlots', () => {
  it('returns plain text as one part', () => {
    expect(splitSlots('od 1. 1. do dnes')).toEqual([{ text: 'od 1. 1. do dnes' }]);
  });

  it('splits around slots in order', () => {
    const text = `od ${slot('from')} do ${slot('to')} · 9 pozic`;
    expect(splitSlots(text)).toEqual([
      { text: 'od ' },
      { slot: 'from' },
      { text: ' do ' },
      { slot: 'to' },
      { text: ' · 9 pozic' },
    ]);
  });

  it('handles a slot at either end and drops empty text', () => {
    expect(splitSlots(`${slot('a')}${slot('b')}`)).toEqual([{ slot: 'a' }, { slot: 'b' }]);
    expect(splitSlots('')).toEqual([]);
  });

  it('keeps an unclosed marker as text', () => {
    expect(splitSlots('ab')).toEqual([{ text: 'ab' }]);
  });
});
