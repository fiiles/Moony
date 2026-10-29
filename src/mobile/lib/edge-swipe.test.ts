import { describe, expect, it } from 'vitest';
import { EDGE_PX, edgeSwipeCommits, edgeSwipeDirection, edgeSwipeStarts } from './edge-swipe';

describe('edgeSwipeStarts', () => {
  it('takes a finger that lands on the left edge only', () => {
    expect(edgeSwipeStarts(0)).toBe(true);
    expect(edgeSwipeStarts(EDGE_PX)).toBe(true);
    expect(edgeSwipeStarts(EDGE_PX + 1)).toBe(false);
  });
});

describe('edgeSwipeDirection', () => {
  it('waits until the finger has moved past the slop', () => {
    expect(edgeSwipeDirection(4, 3)).toBeNull();
  });

  it('follows a drag to the right that is mostly sideways', () => {
    expect(edgeSwipeDirection(14, 6)).toBe('back');
  });

  it('leaves a vertical drag or a drag to the left to the page', () => {
    expect(edgeSwipeDirection(6, 14)).toBe('none');
    expect(edgeSwipeDirection(-14, 2)).toBe('none');
  });
});

describe('edgeSwipeCommits', () => {
  it('goes back past a third of the width', () => {
    expect(edgeSwipeCommits(140, 0.1, 402)).toBe(true);
    expect(edgeSwipeCommits(120, 0.1, 402)).toBe(false);
  });

  it('goes back on a quick flick that has travelled a little', () => {
    expect(edgeSwipeCommits(60, 0.6, 402)).toBe(true);
    expect(edgeSwipeCommits(20, 0.9, 402)).toBe(false);
  });

  it('stays when the flick ends towards the edge again', () => {
    expect(edgeSwipeCommits(200, -0.4, 402)).toBe(false);
  });
});
