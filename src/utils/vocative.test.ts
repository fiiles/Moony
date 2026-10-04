import { describe, expect, it } from 'vitest';
import { vocative } from './vocative';

describe('vocative (Czech greeting)', () => {
  it('leaves other languages alone', () => {
    expect(vocative('Filip', 'en')).toBe('Filip');
    expect(vocative('Jana', 'en-US')).toBe('Jana');
    expect(vocative('  Filip ', 'en')).toBe('Filip');
  });

  it('declines regular masculine names', () => {
    expect(vocative('Filip', 'cs')).toBe('Filipe');
    expect(vocative('David', 'cs-CZ')).toBe('Davide');
    expect(vocative('Robert', 'cs')).toBe('Roberte');
    expect(vocative('Tomáš', 'cs')).toBe('Tomáši');
    expect(vocative('Lukáš', 'cs')).toBe('Lukáši');
    expect(vocative('Ondřej', 'cs')).toBe('Ondřeji');
    expect(vocative('Patrik', 'cs')).toBe('Patriku');
    expect(vocative('Vavřinec', 'cs')).toBe('Vavřinče');
  });

  it('handles -r after a consonant and after a vowel', () => {
    expect(vocative('Petr', 'cs')).toBe('Petře');
    expect(vocative('Igor', 'cs')).toBe('Igore');
    expect(vocative('Oskar', 'cs')).toBe('Oskare');
  });

  it('declines feminine names ending in -a and keeps -e / -ie', () => {
    expect(vocative('Jana', 'cs')).toBe('Jano');
    expect(vocative('Petra', 'cs')).toBe('Petro');
    expect(vocative('Marie', 'cs')).toBe('Marie');
    expect(vocative('Lucie', 'cs')).toBe('Lucie');
  });

  it('knows the common irregular names', () => {
    expect(vocative('Pavel', 'cs')).toBe('Pavle');
    expect(vocative('Karel', 'cs')).toBe('Karle');
    expect(vocative('Zdeněk', 'cs')).toBe('Zdeňku');
    expect(vocative('Marek', 'cs')).toBe('Marku');
    expect(vocative('Jiří', 'cs')).toBe('Jiří');
    expect(vocative('Alex', 'cs')).toBe('Alexi');
  });

  it('keeps names it cannot decline and double names', () => {
    expect(vocative('Dagmar', 'cs')).toBe('Dagmar');
    expect(vocative('Jan Pavel', 'cs')).toBe('Jane Pavel');
    expect(vocative('', 'cs')).toBe('');
  });
});
