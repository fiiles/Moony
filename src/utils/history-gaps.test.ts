import { describe, expect, it } from 'vitest';
import { bridgeUnknownClasses } from './history-gaps';

describe('bridgeUnknownClasses', () => {
  const known = { savings: true, crypto: true, gold: false };

  it('keeps a day whose classes are all known', () => {
    expect(bridgeUnknownClasses({ savings: 10, crypto: 5, gold: 2 }, known)).toEqual({
      savings: 10,
      crypto: 5,
      gold: 2,
    });
  });

  it('leaves out a day that lacks a class today has', () => {
    expect(bridgeUnknownClasses({ savings: 10, crypto: null, gold: 2 }, known)).toBeNull();
  });

  it('counts a class unknown today as well as 0', () => {
    expect(bridgeUnknownClasses({ savings: 10, crypto: 5, gold: null }, known)).toEqual({
      savings: 10,
      crypto: 5,
      gold: 0,
    });
  });
});
