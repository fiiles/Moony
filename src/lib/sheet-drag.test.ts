import { describe, expect, it } from 'vitest';
import {
  releaseVelocity,
  sheetDragCommits,
  sheetDragDirection,
  sheetDragOffset,
} from './sheet-drag';

describe('sheetDragDirection', () => {
  it('waits until the finger has moved past the slop', () => {
    expect(sheetDragDirection(2, 5)).toBeNull();
  });

  it('takes a drag that is mostly downwards', () => {
    expect(sheetDragDirection(3, 14)).toBe('close');
  });

  it('leaves a drag upwards or sideways to the content', () => {
    expect(sheetDragDirection(1, -14)).toBe('none');
    expect(sheetDragDirection(16, 9)).toBe('none');
  });
});

describe('sheetDragOffset', () => {
  it('follows the finger down', () => {
    expect(sheetDragOffset(120)).toBe(120);
  });

  it('resists above the resting place instead of lifting the sheet', () => {
    expect(sheetDragOffset(-40)).toBeLessThan(0);
    expect(sheetDragOffset(-40)).toBeGreaterThan(-40);
    expect(sheetDragOffset(-400)).toBeGreaterThanOrEqual(-24);
  });
});

describe('sheetDragCommits', () => {
  it('closes past a quarter of the sheet height', () => {
    expect(sheetDragCommits(160, 0.1, 600)).toBe(true);
    expect(sheetDragCommits(140, 0.1, 600)).toBe(false);
  });

  it('closes on a quick flick down that has travelled a little', () => {
    expect(sheetDragCommits(50, 0.7, 600)).toBe(true);
    expect(sheetDragCommits(15, 1.2, 600)).toBe(false);
  });

  it('stays when the finger ends moving up again', () => {
    expect(sheetDragCommits(300, -0.3, 600)).toBe(false);
  });
});

describe('releaseVelocity', () => {
  it('keeps the last move velocity for a finger that lifts while moving', () => {
    expect(releaseVelocity(0.8, 1_000, 1_030)).toBe(0.8);
  });

  it('drops it for a finger that rested before lifting', () => {
    expect(releaseVelocity(0.8, 1_000, 1_300)).toBe(0);
  });
});
