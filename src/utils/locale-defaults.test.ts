import { describe, expect, it } from 'vitest';
import {
  RULE_PACK_COUNTRY_IDS,
  countryOptionsFromPacks,
  defaultCurrencyForRegion,
  packIdForRegion,
  regionFromLocale,
} from './locale-defaults';

describe('regionFromLocale', () => {
  it('reads the region subtag and infers it for bare languages', () => {
    expect(regionFromLocale('cs-CZ')).toBe('CZ');
    expect(regionFromLocale('en_GB')).toBe('GB');
    expect(regionFromLocale('de')).toBe('DE');
    expect(regionFromLocale(undefined)).toBeNull();
  });
});

describe('defaultCurrencyForRegion', () => {
  it('maps regions to supported currencies and the euro area to EUR', () => {
    expect(defaultCurrencyForRegion('CZ')).toBe('CZK');
    expect(defaultCurrencyForRegion('US')).toBe('USD');
    expect(defaultCurrencyForRegion('DE')).toBe('EUR');
    expect(defaultCurrencyForRegion('SK')).toBe('EUR');
    expect(defaultCurrencyForRegion('GB')).toBe('GBP');
  });

  it('falls back for regions without a supported currency', () => {
    expect(defaultCurrencyForRegion('PL')).toBe('PLN');
    expect(defaultCurrencyForRegion('HU')).toBe('HUF');
    expect(defaultCurrencyForRegion('UY')).toBe('EUR');
    expect(defaultCurrencyForRegion('UY', 'USD')).toBe('USD');
    expect(defaultCurrencyForRegion(null)).toBe('EUR');
  });
});

describe('countryOptionsFromPacks', () => {
  it('names the shipped packs in the UI language, skips global, sorts by name', () => {
    const options = countryOptionsFromPacks(['global', 'de', 'cz', 'us'], 'en');
    expect(options.map((o) => o.packId)).toEqual(['cz', 'de', 'us']);
    expect(options.find((o) => o.packId === 'de')?.name).toBe('Germany');
    const czech = countryOptionsFromPacks(['de', 'cz'], 'cs');
    expect(czech.find((o) => o.packId === 'de')?.name).toBe('Německo');
  });
});

describe('packIdForRegion', () => {
  it('returns the pack id only when a pack exists', () => {
    expect(packIdForRegion('CZ', ['cz', 'de'])).toBe('cz');
    expect(packIdForRegion('BR', ['cz', 'de'])).toBeNull();
    expect(packIdForRegion(null, ['cz'])).toBeNull();
  });
});

describe('RULE_PACK_COUNTRY_IDS', () => {
  it('matches the shipped rule packs', () => {
    const shipped = Object.keys(import.meta.glob('../../src-tauri/resources/rules/*.json'))
      .map((path) =>
        path
          .split('/')
          .pop()!
          .replace(/\.json$/, '')
      )
      .filter((id) => id !== 'global')
      .sort();
    expect([...RULE_PACK_COUNTRY_IDS]).toEqual(shipped);
  });
});
