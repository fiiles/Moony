import { describe, expect, it } from 'vitest';
import { CHANGE_MARK, changeDirection, splitOnMark } from './change';

describe('changeDirection', () => {
  it('reads the sign of a ratio', () => {
    expect(changeDirection(0.0042, 2)).toBe('up');
    expect(changeDirection(-0.131, 1)).toBe('down');
  });

  it('is flat when the value rounds to zero at the shown precision', () => {
    expect(changeDirection(0, 1)).toBe('flat');
    expect(changeDirection(0.00004, 1)).toBe('flat');
    expect(changeDirection(-0.00004, 1)).toBe('flat');
    expect(changeDirection(0.00004, 3)).toBe('up');
  });

  it('is flat for a missing or non-finite value', () => {
    expect(changeDirection(null, 1)).toBe('flat');
    expect(changeDirection(undefined, 1)).toBe('flat');
    expect(changeDirection(Number.NaN, 1)).toBe('flat');
  });
});

describe('splitOnMark', () => {
  it('splits a translated note around the percent placeholder', () => {
    expect(splitOnMark(`${CHANGE_MARK} oproti vloženému`)).toEqual(['', 'oproti vloženému']);
    expect(splitOnMark(`vs. invested ${CHANGE_MARK}`)).toEqual(['vs. invested', '']);
    expect(splitOnMark(`a ${CHANGE_MARK} b`)).toEqual(['a', 'b']);
  });

  it('returns null when the translation dropped the placeholder', () => {
    expect(splitOnMark('oproti vloženému')).toBeNull();
  });
});
