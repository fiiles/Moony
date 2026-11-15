import { describe, expect, it } from 'vitest';
import { reconcileLightbox } from './lightbox-photos';

describe('reconcileLightbox', () => {
  it('changes nothing when every photo is still there', () => {
    expect(reconcileLightbox(['a', 'b', 'c'], new Set(['c', 'a', 'b']), 1)).toEqual({
      ids: ['a', 'b', 'c'],
      index: 1,
      close: false,
    });
  });

  it('moves to the next photo when the current one is removed', () => {
    expect(reconcileLightbox(['a', 'b', 'c'], new Set(['a', 'c']), 1)).toEqual({
      ids: ['a', 'c'],
      index: 1,
      close: false,
    });
  });

  it('moves to the last photo when the current last one is removed', () => {
    expect(reconcileLightbox(['a', 'b', 'c'], new Set(['a', 'b']), 2)).toEqual({
      ids: ['a', 'b'],
      index: 1,
      close: false,
    });
  });

  it('keeps the same photo open when an earlier one is removed', () => {
    expect(reconcileLightbox(['a', 'b', 'c'], new Set(['b', 'c']), 2)).toEqual({
      ids: ['b', 'c'],
      index: 1,
      close: false,
    });
  });

  it('closes when nothing is left', () => {
    expect(reconcileLightbox(['a', 'b'], new Set(), 0)).toEqual({ ids: [], index: 0, close: true });
  });
});
