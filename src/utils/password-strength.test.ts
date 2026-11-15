import { describe, expect, it } from 'vitest';
import { MIN_PASSWORD_LENGTH } from '@shared/schema';
import { passwordStrength } from './password-strength';

describe('passwordStrength', () => {
  it('says nothing about an empty field', () => {
    expect(passwordStrength('')).toBeNull();
  });

  it('is weak below the minimum length, however varied', () => {
    expect(MIN_PASSWORD_LENGTH).toBe(8);
    expect(passwordStrength('a')).toBe('weak');
    expect(passwordStrength('aB3$xY7')).toBe('weak');
  });

  it('is fair from the minimum length on', () => {
    expect(passwordStrength('abcdefgh')).toBe('fair');
    expect(passwordStrength('aB3$xY7z')).toBe('fair');
  });

  it('is strong at 14 characters with three kinds of characters', () => {
    expect(passwordStrength('abcdefghijKLM1')).toBe('strong');
    expect(passwordStrength('abcdefghij$$$$')).toBe('fair');
    expect(passwordStrength('abcdefghiJKL1')).toBe('fair');
  });

  it('counts lower case, upper case, digits and symbols as the kinds', () => {
    expect(passwordStrength('abcdefghijklm1$')).toBe('strong');
    expect(passwordStrength('ABCDEFGHIJKLM1$')).toBe('strong');
    // A space is no symbol
    expect(passwordStrength('abcdefg hijkl1')).toBe('fair');
  });
});
