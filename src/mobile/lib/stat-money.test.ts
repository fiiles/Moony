import { describe, expect, it } from 'vitest';
import { statMoneyOptions, statTone } from './stat-money';

describe('statMoneyOptions', () => {
  it('shows whole amounts under a million', () => {
    expect(statMoneyOptions(214_380)).toEqual({ decimals: 0 });
    expect(statMoneyOptions(-999_999)).toEqual({ decimals: 0 });
  });

  it('goes compact with two decimals from a million, by magnitude, and takes a sign by spreading', () => {
    expect(statMoneyOptions(1_054_300)).toEqual({ compact: true, decimals: 2 });
    expect(statMoneyOptions(-6_780_000)).toEqual({ compact: true, decimals: 2 });
    expect({ ...statMoneyOptions(1_000_000), signed: true }).toEqual({
      compact: true,
      decimals: 2,
      signed: true,
    });
  });
});

describe('statTone', () => {
  it('has no tone at zero or for an unknown result', () => {
    expect(statTone(null)).toBeUndefined();
    expect(statTone(0)).toBeUndefined();
  });

  it('is green above zero and red under it', () => {
    expect(statTone(2)).toBe('gain');
    expect(statTone(-1)).toBe('loss');
  });
});
