import { describe, expect, it } from 'vitest';
import type { MenuPreferences } from '@shared/schema';
import { areasFromPreferences, preferencesWithAreas } from './tracked-areas';

const allTrue: MenuPreferences = {
  loans: true,
  insurance: true,
  investments: true,
  bonds: true,
  crypto: true,
  otherAssets: true,
  realEstate: true,
};
const allOn = { investments: true, realEstate: true, liabilities: true };

describe('areasFromPreferences', () => {
  it('has everything on without preferences', () => {
    expect(areasFromPreferences(null)).toEqual(allOn);
    expect(areasFromPreferences(undefined)).toEqual(allOn);
  });

  it('keeps investments on while any of its flags is on', () => {
    const prefs = { ...allTrue, investments: false, bonds: false, crypto: true };
    expect(areasFromPreferences(prefs).investments).toBe(true);
  });

  it('turns investments off when all three flags are off', () => {
    const prefs = { ...allTrue, investments: false, bonds: false, crypto: false };
    expect(areasFromPreferences(prefs).investments).toBe(false);
  });

  it('keeps liabilities on while loans or insurance is on', () => {
    expect(areasFromPreferences({ ...allTrue, loans: false, insurance: true }).liabilities).toBe(
      true
    );
    expect(areasFromPreferences({ ...allTrue, loans: false, insurance: false }).liabilities).toBe(
      false
    );
  });
});

describe('preferencesWithAreas', () => {
  it('writes each group to its flags and keeps otherAssets', () => {
    const result = preferencesWithAreas(
      { ...allTrue, otherAssets: false },
      { investments: false, realEstate: true, liabilities: false }
    );
    expect(result).toEqual({
      investments: false,
      bonds: false,
      crypto: false,
      loans: false,
      insurance: false,
      realEstate: true,
      otherAssets: false,
    });
  });

  it('defaults otherAssets to on without current preferences', () => {
    expect(preferencesWithAreas(null, allOn)).toEqual(allTrue);
  });
});
