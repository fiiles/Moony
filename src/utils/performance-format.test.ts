import { describe, expect, it } from 'vitest';
import { createFormatters } from '@/lib/format';
import { arrowOf, formatPoints, toneOf } from './performance-format';

const cs = createFormatters('cs-CZ', 'CZK');
const en = createFormatters('en-US', 'USD');

describe('toneOf', () => {
  it('colours by the sign of the displayed value', () => {
    expect(toneOf(2.3)).toBe('gain');
    expect(toneOf(-0.1)).toBe('loss');
    expect(toneOf(0)).toBe('flat');
  });

  it('is flat when the value rounds to zero at the shown precision', () => {
    expect(toneOf(0.04)).toBe('flat');
    expect(toneOf(-0.049)).toBe('flat');
    expect(toneOf(0.05)).toBe('gain');
    expect(toneOf(0.04, 2)).toBe('gain');
  });

  it('is flat for missing or non-finite values', () => {
    expect(toneOf(null)).toBe('flat');
    expect(toneOf(undefined)).toBe('flat');
    expect(toneOf(NaN)).toBe('flat');
  });
});

describe('arrowOf', () => {
  it('maps a tone to its glyph', () => {
    expect(arrowOf('gain')).toBe('▲ ');
    expect(arrowOf('loss')).toBe('▼ ');
    expect(arrowOf('flat')).toBe('');
  });
});

describe('formatPoints', () => {
  it('writes a signed change with a true minus and no-break spaces', () => {
    expect(formatPoints(cs.number, 2.46, 'p. b.')).toBe('+ 2,5 p. b.');
    expect(formatPoints(cs.number, -0.84, 'p. b.')).toBe('− 0,8 p. b.');
    expect(formatPoints(en.number, 12.04, 'pp')).toBe('+ 12.0 pp');
  });

  it('drops the sign when the value rounds to zero', () => {
    expect(formatPoints(cs.number, 0.04, 'p. b.')).toBe('0,0 p. b.');
    expect(formatPoints(cs.number, -0.04, 'p. b.')).toBe('0,0 p. b.');
    expect(formatPoints(cs.number, 0, 'p. b.')).toBe('0,0 p. b.');
  });

  it('gives the magnitude alone when asked', () => {
    expect(formatPoints(cs.number, -3.21, 'p. b.', { signed: false })).toBe('3,2 p. b.');
  });

  it('keeps the thousands separator of the locale', () => {
    expect(formatPoints(en.number, 1234.5, 'pp')).toBe('+ 1,234.5 pp');
  });
});
