import { describe, expect, it } from 'vitest';
import type { CsvPreset, MobileExportGuide } from '@shared/schema';
import { guideBankName, guideForAccount, guideSteps, guidesList } from './export-guide';

const guide = (steps: Record<string, string[]>, over: Partial<MobileExportGuide> = {}) =>
  ({
    route: 'web',
    format: 'CSV',
    steps,
    appUrlScheme: null,
    webUrl: 'https://bank.example/',
    ...over,
  }) satisfies MobileExportGuide;

const preset = (over: Partial<CsvPreset>): CsvPreset =>
  ({
    id: 'x',
    bankName: 'X',
    country: 'CZ',
    institutionId: null,
    mobileExport: guide({ en: ['a', 'b'] }),
    ...over,
  }) as CsvPreset;

describe('guideSteps', () => {
  const steps = {
    en: ['Sign in', 'Export', 'Share'],
    cs: ['Přihlaste se', 'Exportujte', 'Sdílejte'],
    de: ['Anmelden', 'Exportieren', 'Teilen'],
  };

  it('gives the steps of the language', () => {
    expect(guideSteps(guide(steps), 'cs')).toEqual(['Přihlaste se', 'Exportujte', 'Sdílejte']);
    expect(guideSteps(guide(steps), 'de')).toEqual(['Anmelden', 'Exportieren', 'Teilen']);
  });

  it('reads a regional tag as its language', () => {
    expect(guideSteps(guide(steps), 'de-AT')).toEqual(['Anmelden', 'Exportieren', 'Teilen']);
    expect(guideSteps(guide(steps), 'cs_CZ')[0]).toBe('Přihlaste se');
  });

  it('falls back to English for a language the guide does not have', () => {
    expect(guideSteps(guide(steps), 'pl')).toEqual(['Sign in', 'Export', 'Share']);
    expect(guideSteps(guide(steps), 'fr')).toEqual(['Sign in', 'Export', 'Share']);
    expect(guideSteps(guide(steps), '')).toEqual(['Sign in', 'Export', 'Share']);
  });

  it('falls back to English for a language whose steps are empty', () => {
    expect(guideSteps(guide({ ...steps, cs: [] }), 'cs')).toEqual(['Sign in', 'Export', 'Share']);
  });

  it('is empty when there is not even English', () => {
    expect(guideSteps(guide({ cs: ['Jen česky'] }), 'de')).toEqual([]);
  });
});

describe('guideForAccount', () => {
  const fio = preset({ id: 'fio', bankName: 'Fio banka', institutionId: 'inst_fio' });
  const revolut = preset({ id: 'revolut', bankName: 'Revolut', institutionId: 'inst_revolut' });
  const mbank = preset({ id: 'mbank', bankName: 'mBank' });

  it('finds the guide of the account’s bank by its institution', () => {
    expect(guideForAccount([mbank, fio, revolut], { institutionId: 'inst_revolut' })?.id).toBe(
      'revolut'
    );
  });

  it('is null for an account without an institution, an unknown one, or no account at all', () => {
    expect(guideForAccount([fio], { institutionId: null })).toBeNull();
    expect(guideForAccount([fio], { institutionId: 'inst_other' })).toBeNull();
    expect(guideForAccount([fio], undefined)).toBeNull();
    expect(guideForAccount([fio], null)).toBeNull();
    expect(guideForAccount([], { institutionId: 'inst_fio' })).toBeNull();
  });

  it('is null when the bank’s preset carries no guide', () => {
    const bare = preset({ id: 'fio', institutionId: 'inst_fio', mobileExport: null });
    expect(guideForAccount([bare], { institutionId: 'inst_fio' })).toBeNull();
  });
});

describe('guidesList', () => {
  it('lists only the banks that have a guide, by country and then by name', () => {
    const list = guidesList([
      preset({ id: 'wise', bankName: 'Wise', country: 'GB' }),
      preset({ id: 'sparkasse', bankName: 'Sparkasse', country: 'DE' }),
      preset({ id: 'cs', bankName: 'Česká spořitelna', country: 'CZ' }),
      preset({ id: 'uk', bankName: 'UK banks', country: 'GB', mobileExport: null }),
      preset({ id: 'air', bankName: 'Air Bank', country: 'CZ' }),
      preset({ id: 'csob', bankName: 'ČSOB', country: 'CZ' }),
      preset({ id: 'dkb', bankName: 'DKB', country: 'DE' }),
    ]);
    expect(list.map((p) => p.id)).toEqual(['air', 'cs', 'csob', 'dkb', 'sparkasse', 'wise']);
  });

  it('does not reorder what it was given', () => {
    const given = [
      preset({ id: 'b', bankName: 'B', country: 'DE' }),
      preset({ id: 'a', bankName: 'A', country: 'CZ' }),
    ];
    guidesList(given);
    expect(given.map((p) => p.id)).toEqual(['b', 'a']);
  });

  it('is empty without presets', () => {
    expect(guidesList([])).toEqual([]);
  });
});

describe('guideBankName', () => {
  it('drops the country the data adds to a name ("Chase (US)")', () => {
    expect(guideBankName('Chase (US)')).toBe('Chase');
    expect(guideBankName('ING (Germany)')).toBe('ING');
  });

  it('keeps a name that has none', () => {
    expect(guideBankName('Fio banka')).toBe('Fio banka');
    expect(guideBankName('MONETA Money Bank')).toBe('MONETA Money Bank');
  });

  it('keeps the name whole when nothing else would be left', () => {
    expect(guideBankName('(US)')).toBe('(US)');
  });
});
