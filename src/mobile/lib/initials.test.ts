import { describe, expect, it } from 'vitest';
import { initials } from './initials';

describe('initials', () => {
  it('takes the first letters of the first two words', () => {
    expect(initials('Lidl Praha 4')).toBe('LP');
    expect(initials('jan novák')).toBe('JN');
  });

  it('takes two letters of a single word', () => {
    expect(initials('Netflix')).toBe('NE');
  });

  it('skips words without a letter', () => {
    expect(initials('4 Ever Shop')).toBe('ES');
    expect(initials('Umbrella a.s.')).toBe('UA');
  });

  it('keeps letters of any script', () => {
    expect(initials('Česká spořitelna')).toBe('ČS');
    expect(initials('楽天')).toBe('楽天');
  });

  it('is empty without letters', () => {
    expect(initials('')).toBe('');
    expect(initials('123 456')).toBe('');
  });
});
