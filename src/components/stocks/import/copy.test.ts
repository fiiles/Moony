import { describe, expect, it } from 'vitest';
import csCommon from '@/i18n/locales/cs/common.json';
import csStocks from '@/i18n/locales/cs/stocks.json';
import enCommon from '@/i18n/locales/en/common.json';
import enStocks from '@/i18n/locales/en/stocks.json';
import { SOURCE_IDS, SOURCE_HELP_URLS, sourceHelpUrl } from './import-config';

/**
 * The wizard's copy lives in both locales (docs/standards/i18n.md); nothing else catches a key
 * that exists in one language only. These tests are that gate for `stocks.importWizard`, the
 * guides of every source, the row messages the parsers emit and the validation keys of the
 * Rust config.
 */

type Tree = { [key: string]: string | Tree };

function flatten(tree: Tree, prefix = ''): Record<string, string> {
  const flat: Record<string, string> = {};
  for (const [key, value] of Object.entries(tree)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof value === 'string') flat[path] = value;
    else Object.assign(flat, flatten(value, path));
  }
  return flat;
}

const PLURAL_SUFFIX = /_(zero|one|two|few|many|other)$/;
const cs = flatten((csStocks as unknown as { importWizard: Tree }).importWizard);
const en = flatten((enStocks as unknown as { importWizard: Tree }).importWizard);

/** Key without its plural suffix: Czech has one/few/other, English one/other. */
const bases = (flat: Record<string, string>) =>
  [...new Set(Object.keys(flat).map((key) => key.replace(PLURAL_SUFFIX, '')))].sort();

describe('stocks.importWizard copy', () => {
  it('has the same keys in Czech and English', () => {
    expect(bases(cs)).toEqual(bases(en));
  });

  it('gives every plural a form for "other" and Czech one for "few"', () => {
    for (const [flat, language] of [
      [cs, 'cs'],
      [en, 'en'],
    ] as const) {
      const plural = Object.keys(flat).filter((key) => PLURAL_SUFFIX.test(key));
      for (const key of plural) {
        const base = key.replace(PLURAL_SUFFIX, '');
        expect(flat[`${base}_other`], `${language}: ${base}_other`).toBeTruthy();
        expect(flat[`${base}_one`], `${language}: ${base}_one`).toBeTruthy();
        if (language === 'cs') expect(flat[`${base}_few`], `cs: ${base}_few`).toBeTruthy();
      }
    }
  });

  it('has no empty strings and no exclamation marks', () => {
    for (const [flat, language] of [
      [cs, 'cs'],
      [en, 'en'],
    ] as const) {
      for (const [key, value] of Object.entries(flat)) {
        expect(value.trim(), `${language}: ${key}`).not.toBe('');
        expect(value, `${language}: ${key}`).not.toContain('!');
      }
    }
  });

  it('guides every source in both languages', () => {
    for (const flat of [cs, en]) {
      for (const id of SOURCE_IDS) {
        expect(flat[`sources.${id}.name`], id).toBeTruthy();
        expect(flat[`sources.${id}.guideTitle`], id).toBeTruthy();
        expect(flat[`sources.${id}.guide`], id).toBeTruthy();
      }
      for (const column of ['date', 'type', 'symbol', 'quantity', 'price', 'currency', 'name']) {
        expect(flat[`sources.moony.columns.${column}.label`], column).toBeTruthy();
        expect(flat[`sources.moony.columns.${column}.hint`], column).toBeTruthy();
      }
    }
  });

  it('translates every row message the parsers and the simulation emit', () => {
    const keys = [
      'notATrade',
      'zeroQuantity',
      'assetClass',
      'instrumentSkipped',
      'dateUnparseable',
      'numberUnparseable',
      'commentUnparseable',
      'symbolMissing',
      'symbolMissingIsin',
      'tickerInvalid',
      'currencyMissing',
      'currencyInvalid',
      'priceMissing',
      'cannotParse',
      'writeFailed',
      'sellExceedsHoldings',
      'sellExceedsHoldingsHeld',
      'currencyMismatch',
      'currencyMismatchPosition',
      'duplicate',
      'duplicateById',
    ];
    for (const flat of [cs, en]) {
      for (const key of keys) expect(flat[`row.${key}`], key).toBeTruthy();
    }
    // The sentences that work the detail in must have a place for it.
    for (const flat of [cs, en]) {
      expect(flat['row.sellExceedsHoldingsHeld']).toContain('{{detail}}');
      expect(flat['row.currencyMismatchPosition']).toContain('{{detail}}');
      expect(flat['row.symbolMissingIsin']).toContain('{{detail}}');
    }
  });

  it('translates the validation keys the stock import commands can return', () => {
    const keys = [
      'csvEmptyFile',
      'nameRequired',
      'nameTooLong',
      'sellExceedsHoldings',
      'csvDelimiterInvalid',
      'csvDecimalSeparatorInvalid',
      'dateFormatRequired',
      'stockImportSymbolRequired',
      'currencyInvalid',
      'stockImportCurrencyRequired',
      'stockImportTypeRequired',
      'tickerInvalid',
    ];
    for (const common of [csCommon, enCommon]) {
      const validation = common.validation as Record<string, string>;
      for (const key of keys) expect(validation[key], key).toBeTruthy();
    }
  });

  it('explains GBX, which says nothing by itself, wherever it is shown', () => {
    for (const flat of [cs, en]) {
      expect(flat['currency.gbx']).toContain('GBX');
      expect(flat['currency.gbx']).toContain('GBP ÷ 100');
      expect(flat['editor.currencyFromFile']).toBeTruthy();
    }
  });

  it('keeps the Czech guides of the brokers short enough to read at a glance', () => {
    for (const id of ['xtb', 'trading212', 'degiro', 'ibkr', 'moony', 'custom']) {
      expect(cs[`sources.${id}.guide`].length, id).toBeLessThan(420);
    }
  });
});

describe('help links', () => {
  const brokers = SOURCE_IDS.filter((id) => id !== 'moony' && id !== 'custom');

  it('gives every broker a link to its own help page in both languages', () => {
    expect(Object.keys(SOURCE_HELP_URLS).sort()).toEqual([...brokers].sort());
    for (const id of brokers) {
      for (const language of ['cs', 'en']) {
        expect(sourceHelpUrl(id, language), `${id} ${language}`).toMatch(/^https:\/\//);
      }
    }
  });
});
