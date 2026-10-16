import { describe, expect, it } from 'vitest';
import { SWIPE_MAX, SWIPE_TRIGGER, swipeAction, swipeOffset } from './swipe';

describe('swipeOffset', () => {
  const both = { left: true, right: true };

  it('follows the finger up to the action width', () => {
    expect(swipeOffset(40, both)).toBe(40);
    expect(swipeOffset(-40, both)).toBe(-40);
    expect(swipeOffset(160, both)).toBe(SWIPE_MAX);
    expect(swipeOffset(-160, both)).toBe(-SWIPE_MAX);
  });

  it('does not move towards a side without an action', () => {
    expect(swipeOffset(50, { left: true, right: false })).toBe(0);
    expect(swipeOffset(-50, { left: false, right: true })).toBe(0);
  });
});

describe('swipeAction', () => {
  it('fires once the row has moved past the trigger', () => {
    expect(swipeAction(SWIPE_TRIGGER)).toBe('right');
    expect(swipeAction(-SWIPE_TRIGGER)).toBe('left');
    expect(swipeAction(SWIPE_MAX)).toBe('right');
  });

  it('snaps back without an action short of it', () => {
    expect(swipeAction(SWIPE_TRIGGER - 1)).toBeNull();
    expect(swipeAction(-(SWIPE_TRIGGER - 1))).toBeNull();
    expect(swipeAction(0)).toBeNull();
  });
});
