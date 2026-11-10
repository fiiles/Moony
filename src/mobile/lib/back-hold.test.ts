import { describe, expect, it } from 'vitest';
import { backIsHeld, holdBack } from './back-hold';

describe('holdBack', () => {
  it('holds back until every hold is released, whatever the order', () => {
    expect(backIsHeld()).toBe(false);
    const overlay = holdBack();
    const cover = holdBack();
    expect(backIsHeld()).toBe(true);
    overlay();
    expect(backIsHeld()).toBe(true);
    cover();
    expect(backIsHeld()).toBe(false);
  });

  it('counts a release once, however often it is called', () => {
    const first = holdBack();
    const second = holdBack();
    first();
    first();
    expect(backIsHeld()).toBe(true);
    second();
    expect(backIsHeld()).toBe(false);
  });
});
