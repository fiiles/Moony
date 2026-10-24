import { describe, expect, it } from 'vitest';
import { FIT_MIN_PX, fitSizeFor } from './fit-size';

describe('fitSizeFor', () => {
  it('leaves 2 % of the room and steps down to half a pixel', () => {
    // 17 px text is exactly as wide as its room: 17 * 0.98 = 16.66 -> 16.5
    expect(fitSizeFor(100, 100, 17)).toBe(16.5);
  });

  it('is the size at which the text just fits when there is room to spare', () => {
    // twice the room: 17 * 2 * 0.98 = 33.32 -> 33
    expect(fitSizeFor(200, 100, 17)).toBe(33);
  });

  it('measures from the size the text is drawn at, whatever it is', () => {
    // the same text drawn at 18.7 px is 10 % wider: the answer does not depend on where it was measured
    expect(fitSizeFor(97, 100.8, 18.7)).toBe(fitSizeFor(97, (100.8 * 17) / 18.7, 17));
  });

  it('never returns a size at which the text, drawn there, is wider than its room', () => {
    // the fractional room 96.67 of a 97 px (rounded) box with 97 px of text at 18 px: below 18
    const size = fitSizeFor(96.67, 97, 18);
    expect(size).not.toBeNull();
    expect((97 / 18) * (size as number)).toBeLessThanOrEqual(96.67);
  });

  it('keeps a floor, so a figure far too long for its box does not vanish', () => {
    expect(fitSizeFor(10, 100, 17)).toBe(FIT_MIN_PX);
  });

  it('is null while there is nothing to measure', () => {
    expect(fitSizeFor(0, 100, 17)).toBeNull();
    expect(fitSizeFor(100, 0, 17)).toBeNull();
    expect(fitSizeFor(100, 100, 0)).toBeNull();
    expect(fitSizeFor(Number.NaN, 100, 17)).toBeNull();
  });
});
