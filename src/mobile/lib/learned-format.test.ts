import { describe, expect, it, vi } from 'vitest';
import type { InsertLearnedCsvFormat, LearnedCsvFormat, LearnedCsvMapping } from '@shared/schema';
import {
  EMPTY_MAPPING,
  type MappingState,
} from '@/components/bank-accounts/csv-import/import-config';
import {
  applyLearning,
  defaultFormatName,
  learnedFormatInsert,
  learnedFormatSub,
  learnedMappingFrom,
  learnedPreselect,
  learningAfterImport,
  mappingForFile,
  normalizeHeader,
} from './learned-format';

const DAY = 86_400;
/** UTC midnight of the "today" the tests run on (2026-10-25). */
const TODAY = Math.floor(Date.UTC(2026, 9, 25) / 1000);

const file = (headers: string[]) => ({ headers });

const stored = (overrides: Partial<LearnedCsvMapping> = {}): LearnedCsvMapping => ({
  dateColumn: 'datum',
  dateFormat: '%d.%m.%Y',
  amountColumn: 'částka',
  debitColumn: null,
  creditColumn: null,
  balanceColumn: null,
  transactionIdColumn: null,
  descriptionColumns: [],
  counterpartyColumn: null,
  counterpartyIbanColumn: null,
  counterpartyBankCodeColumn: null,
  counterpartyCountryColumn: null,
  currencyColumn: null,
  decimalSeparator: ',',
  ...overrides,
});

const format = (overrides: Partial<LearnedCsvFormat> = {}): LearnedCsvFormat => ({
  id: 'learned:1',
  name: 'Moje banka',
  signature: 'v1',
  headers: ['Datum', 'Částka', 'Poznámka'],
  delimiter: ';',
  encoding: 'windows-1250',
  headerRow: 1,
  mapping: stored(),
  accountId: null,
  createdAt: 1,
  lastUsedAt: 1,
  shadowedByPresetId: null,
  ...overrides,
});

describe('normalizeHeader', () => {
  it('equals the normalisation of the layout signature on the Rust side', () => {
    expect(normalizeHeader('\u{feff}Datum ')).toBe('datum');
    expect(normalizeHeader('ČÁSTKA')).toBe('částka');
    expect(normalizeHeader('Datum   zaúčtování')).toBe('datum zaúčtování');
    expect(normalizeHeader('  datum zaúčtování ')).toBe('datum zaúčtování');
    expect(normalizeHeader('')).toBe('');
    expect(normalizeHeader('   ')).toBe('');
  });

  it('takes whitespace as Rust does, not as JavaScript does', () => {
    // NEL is whitespace to Rust only, a BOM inside a cell is whitespace to JavaScript only
    expect(normalizeHeader('\u0085Částka\u0085')).toBe('částka');
    expect(normalizeHeader('Částka\u{feff}X')).toBe('částka\u{feff}x');
    expect(normalizeHeader('Datum \t\n zaúčtování')).toBe('datum zaúčtování');
    expect(normalizeHeader('　Datum ')).toBe('datum');
  });

  it('drops a BOM only at the start, whatever whitespace is around it', () => {
    expect(normalizeHeader('\u{feff}\u{feff}Částka')).toBe('částka');
    expect(normalizeHeader(' \u{feff} Datum')).toBe('datum');
  });
});

describe('learnedMappingFrom', () => {
  const mapping: MappingState = {
    ...EMPTY_MAPPING,
    dateColumn: 'Datum',
    dateFormat: '%d.%m.%Y',
    amountColumn: 'Částka',
    descriptionColumns: ['Zpráva', 'Poznámka', 'Název'],
    counterpartyColumn: 'Protistrana',
    decimalSeparator: ',',
    rowFilterEnabled: true,
  };

  it('turns every empty column into null and keeps the others as they are', () => {
    expect(learnedMappingFrom(mapping)).toEqual({
      dateColumn: 'Datum',
      dateFormat: '%d.%m.%Y',
      amountColumn: 'Částka',
      debitColumn: null,
      creditColumn: null,
      balanceColumn: null,
      transactionIdColumn: null,
      descriptionColumns: ['Zpráva', 'Poznámka', 'Název'],
      counterpartyColumn: 'Protistrana',
      counterpartyIbanColumn: null,
      counterpartyBankCodeColumn: null,
      counterpartyCountryColumn: null,
      currencyColumn: null,
      decimalSeparator: ',',
    });
  });

  it('keeps a pair, the order of the description columns and the separator left to detect', () => {
    const learned = learnedMappingFrom({
      ...EMPTY_MAPPING,
      dateColumn: 'Datum',
      debitColumn: 'Výdaje',
      creditColumn: 'Příjmy',
      descriptionColumns: ['B', 'A'],
    });
    expect(learned.debitColumn).toBe('Výdaje');
    expect(learned.creditColumn).toBe('Příjmy');
    expect(learned.amountColumn).toBeNull();
    expect(learned.descriptionColumns).toEqual(['B', 'A']);
    expect(learned.dateFormat).toBe('auto');
    expect(learned.decimalSeparator).toBe('auto');
  });

  it('drops the row filter, which belongs to a preset', () => {
    expect('rowFilterEnabled' in learnedMappingFrom(mapping)).toBe(false);
  });

  it('writes an empty date format as auto', () => {
    expect(learnedMappingFrom({ ...mapping, dateFormat: '' }).dateFormat).toBe('auto');
  });
});

describe('mappingForFile', () => {
  it('resolves every stored column to the file’s own header cell', () => {
    const mapping = mappingForFile(
      format({
        mapping: stored({
          balanceColumn: 'zůstatek',
          descriptionColumns: ['poznámka', 'zpráva'],
          decimalSeparator: '.',
        }),
      }),
      file(['DATUM', 'ČÁSTKA ', ' Zpráva', 'Poznámka', 'Zůstatek'])
    );
    expect(mapping).toEqual({
      ...EMPTY_MAPPING,
      dateColumn: 'DATUM',
      dateFormat: '%d.%m.%Y',
      amountColumn: 'ČÁSTKA ',
      balanceColumn: 'Zůstatek',
      descriptionColumns: ['Poznámka', ' Zpráva'],
      decimalSeparator: '.',
    });
  });

  it('resolves "ČÁSTKA " to the file’s "Částka"', () => {
    const mapping = mappingForFile(
      format({ mapping: stored({ amountColumn: 'ČÁSTKA ' }) }),
      file(['Datum', 'Částka'])
    );
    expect(mapping?.amountColumn).toBe('Částka');
  });

  it('reads a BOM in the file’s first header cell like the signature does', () => {
    const mapping = mappingForFile(format(), file(['\u{feff}Datum', 'Částka']));
    expect(mapping?.dateColumn).toBe('\u{feff}Datum');
  });

  it('prefers the cell that is written exactly as stored', () => {
    const mapping = mappingForFile(
      format({ mapping: stored({ amountColumn: 'Částka' }) }),
      file(['Datum', 'ČÁSTKA', 'Částka'])
    );
    expect(mapping?.amountColumn).toBe('Částka');
  });

  it('is null without a date', () => {
    expect(mappingForFile(format(), file(['Den', 'Částka']))).toBeNull();
    expect(
      mappingForFile(format({ mapping: stored({ dateColumn: '' }) }), file(['Datum', 'Částka']))
    ).toBeNull();
  });

  it('is null without an amount', () => {
    expect(mappingForFile(format(), file(['Datum', 'Obrat']))).toBeNull();
  });

  it('is null with only one side of a debit and credit pair', () => {
    const pair = format({
      mapping: stored({ amountColumn: null, debitColumn: 'výdaje', creditColumn: 'příjmy' }),
    });
    expect(mappingForFile(pair, file(['Datum', 'Výdaje']))).toBeNull();
    expect(mappingForFile(pair, file(['Datum', 'Příjmy']))).toBeNull();
    expect(mappingForFile(pair, file(['Datum', 'Výdaje', 'Příjmy']))).toMatchObject({
      debitColumn: 'Výdaje',
      creditColumn: 'Příjmy',
      amountColumn: '',
    });
  });

  it('leaves out the half of a pair next to an amount', () => {
    const mapping = mappingForFile(
      format({
        mapping: stored({ amountColumn: 'částka', debitColumn: 'výdaje', creditColumn: 'příjmy' }),
      }),
      file(['Datum', 'Částka', 'Výdaje'])
    );
    expect(mapping).toMatchObject({ amountColumn: 'Částka', debitColumn: '', creditColumn: '' });
  });

  it('drops an optional column it cannot resolve and keeps the rest', () => {
    const mapping = mappingForFile(
      format({
        mapping: stored({
          balanceColumn: 'zůstatek',
          currencyColumn: 'měna',
          descriptionColumns: ['zpráva', 'poznámka'],
          counterpartyColumn: 'protistrana',
        }),
      }),
      file(['Datum', 'Částka', 'Měna', 'Poznámka'])
    );
    expect(mapping).toMatchObject({
      dateColumn: 'Datum',
      amountColumn: 'Částka',
      balanceColumn: '',
      currencyColumn: 'Měna',
      counterpartyColumn: '',
      descriptionColumns: ['Poznámka'],
    });
  });

  it('never repeats a description column', () => {
    const mapping = mappingForFile(
      format({ mapping: stored({ descriptionColumns: ['poznámka', 'POZNÁMKA'] }) }),
      file(['Datum', 'Částka', 'Poznámka'])
    );
    expect(mapping?.descriptionColumns).toEqual(['Poznámka']);
  });

  it('reads a date format and a separator it does not know as left to detect', () => {
    const mapping = mappingForFile(
      format({ mapping: stored({ dateFormat: '', decimalSeparator: 'both' }) }),
      file(['Datum', 'Částka'])
    );
    expect(mapping).toMatchObject({ dateFormat: 'auto', decimalSeparator: 'auto' });
  });

  it('never turns the row filter on', () => {
    expect(mappingForFile(format(), file(['Datum', 'Částka']))?.rowFilterEnabled).toBe(false);
  });
});

describe('defaultFormatName', () => {
  it('is the account’s institution first', () => {
    expect(
      defaultFormatName({ name: 'Běžný účet', institution: { name: 'Fio banka' } }, 'export.csv')
    ).toBe('Fio banka');
  });

  it('is the account’s name without an institution', () => {
    expect(defaultFormatName({ name: 'Běžný účet', institution: null }, 'export.csv')).toBe(
      'Běžný účet'
    );
    expect(defaultFormatName({ name: 'Běžný účet' }, 'export.csv')).toBe('Běžný účet');
    expect(
      defaultFormatName({ name: 'Běžný účet', institution: { name: '  ' } }, 'export.csv')
    ).toBe('Běžný účet');
  });

  it('is the file’s name without its extension without an account', () => {
    expect(defaultFormatName(undefined, 'pohyby 2026.csv')).toBe('pohyby 2026');
    expect(defaultFormatName(undefined, 'a.b.csv')).toBe('a.b');
    expect(defaultFormatName(undefined, 'export')).toBe('export');
    expect(defaultFormatName({ name: ' ', institution: null }, 'export.TXT')).toBe('export');
  });

  it('cuts a long name to 60 characters', () => {
    const long = 'B'.repeat(61);
    expect(defaultFormatName({ name: 'x', institution: { name: long } }, 'a.csv')).toBe(
      'B'.repeat(60)
    );
    expect(defaultFormatName(undefined, `${'c'.repeat(70)}.csv`)).toBe('c'.repeat(60));
    // Characters, not UTF-16 units: the store counts characters
    expect(defaultFormatName({ name: '😀'.repeat(61) }, 'a.csv')).toBe('😀'.repeat(60));
    // A cut never leaves a trailing space
    expect(defaultFormatName({ name: `${'d'.repeat(59)} ${'e'.repeat(10)}` }, 'a.csv')).toBe(
      'd'.repeat(59)
    );
  });
});

describe('learnedPreselect', () => {
  const open = (id: string) => ({ id, institutionId: null, terminationDate: null });
  const closed = (id: string) => ({ id, institutionId: null, terminationDate: TODAY - DAY });
  const accounts = [open('a'), open('b'), closed('c')];

  it('is the account the entry point named, whatever the format remembers', () => {
    expect(
      learnedPreselect({
        format: format({ accountId: 'b' }),
        requested: 'a',
        accounts,
        todayDay: TODAY,
      })
    ).toBe('a');
    expect(learnedPreselect({ format: null, requested: 'a', accounts, todayDay: TODAY })).toBe('a');
  });

  it('is the format’s account when it is open', () => {
    expect(
      learnedPreselect({ format: format({ accountId: 'b' }), accounts, todayDay: TODAY })
    ).toBe('b');
  });

  it('is nothing for a deleted or a closed account', () => {
    expect(
      learnedPreselect({ format: format({ accountId: 'gone' }), accounts, todayDay: TODAY })
    ).toBeUndefined();
    expect(
      learnedPreselect({ format: format({ accountId: 'c' }), accounts, todayDay: TODAY })
    ).toBeUndefined();
  });

  it('is nothing for a format without an account, or without a format', () => {
    expect(learnedPreselect({ format: format(), accounts, todayDay: TODAY })).toBeUndefined();
    expect(learnedPreselect({ format: null, accounts, todayDay: TODAY })).toBeUndefined();
  });
});

describe('learningAfterImport', () => {
  const insert: InsertLearnedCsvFormat = {
    name: 'Fio banka',
    headers: ['Datum', 'Částka'],
    delimiter: ';',
    encoding: 'UTF-8',
    headerRow: 0,
    mapping: stored(),
    accountId: 'a',
  };

  it('saves a recognized layout', () => {
    expect(learningAfterImport({ source: 'recognized', learned: null, insert })).toEqual({
      kind: 'save',
      insert,
    });
  });

  it('saves a layout the person changed from a learned format (the recognition replaces it)', () => {
    expect(learningAfterImport({ source: 'recognized', learned: format(), insert })).toEqual({
      kind: 'save',
      insert,
    });
  });

  it('learns nothing from a recognized layout it has no insert for', () => {
    expect(learningAfterImport({ source: 'recognized', learned: null, insert: null })).toEqual({
      kind: 'none',
    });
  });

  it('marks a learned format used', () => {
    expect(learningAfterImport({ source: 'learned', learned: format(), insert: null })).toEqual({
      kind: 'markUsed',
      id: 'learned:1',
    });
    expect(learningAfterImport({ source: 'learned', learned: format(), insert })).toEqual({
      kind: 'markUsed',
      id: 'learned:1',
    });
  });

  it('marks nothing used for a learned source without a format', () => {
    expect(learningAfterImport({ source: 'learned', learned: null, insert: null })).toEqual({
      kind: 'none',
    });
  });

  it('learns nothing from a preset', () => {
    expect(learningAfterImport({ source: 'preset', learned: null, insert })).toEqual({
      kind: 'none',
    });
    expect(learningAfterImport({ source: 'preset', learned: format(), insert })).toEqual({
      kind: 'none',
    });
  });
});

describe('learnedFormatInsert', () => {
  it('carries the file’s layout, the mapping and the account written to', () => {
    const csv = {
      headers: ['Datum', 'Částka', ''],
      delimiter: '\t',
      encoding: 'windows-1250',
      headerRow: 2,
    };
    expect(
      learnedFormatInsert({
        name: 'Fio banka',
        accountId: 'a',
        csv,
        mapping: { ...EMPTY_MAPPING, dateColumn: 'Datum', amountColumn: 'Částka' },
      })
    ).toEqual({
      name: 'Fio banka',
      headers: ['Datum', 'Částka', ''],
      delimiter: '\t',
      encoding: 'windows-1250',
      headerRow: 2,
      mapping: learnedMappingFrom({
        ...EMPTY_MAPPING,
        dateColumn: 'Datum',
        amountColumn: 'Částka',
      }),
      accountId: 'a',
    });
  });
});

describe('applyLearning', () => {
  const insert: InsertLearnedCsvFormat = {
    name: 'Fio banka',
    headers: ['Datum', 'Částka'],
    delimiter: ';',
    encoding: 'UTF-8',
    headerRow: 0,
    mapping: stored(),
    accountId: 'a',
  };
  const store = (
    overrides: { save?: () => Promise<LearnedCsvFormat>; markUsed?: () => Promise<void> } = {}
  ) => ({
    save: vi.fn(overrides.save ?? (() => Promise.resolve(format({ name: 'Fio banka' })))),
    markUsed: vi.fn(overrides.markUsed ?? (() => Promise.resolve())),
  });

  it('saves the layout and resolves with the saved format', async () => {
    const target = store();
    const saved = await applyLearning({ kind: 'save', insert }, target);
    expect(target.save).toHaveBeenCalledWith(insert);
    expect(target.markUsed).not.toHaveBeenCalled();
    expect(saved?.name).toBe('Fio banka');
  });

  it('marks a format used and has nothing to report', async () => {
    const target = store();
    expect(await applyLearning({ kind: 'markUsed', id: 'learned:1' }, target)).toBeNull();
    expect(target.markUsed).toHaveBeenCalledWith('learned:1');
    expect(target.save).not.toHaveBeenCalled();
  });

  it('touches nothing for none', async () => {
    const target = store();
    expect(await applyLearning({ kind: 'none' }, target)).toBeNull();
    expect(target.save).not.toHaveBeenCalled();
    expect(target.markUsed).not.toHaveBeenCalled();
  });

  it('takes the refusal of a header row that is data as "not learned", without an error', async () => {
    const target = store({
      save: () => Promise.reject(new Error('validation.learnedFormatHeaderless')),
    });
    await expect(applyLearning({ kind: 'save', insert }, target)).resolves.toBeNull();
  });

  it('takes any other failure as "not learned" too', async () => {
    const target = store({ save: () => Promise.reject(new Error('disk full')) });
    await expect(applyLearning({ kind: 'save', insert }, target)).resolves.toBeNull();
    const gone = store({ markUsed: () => Promise.reject(new Error('Learned format not found')) });
    await expect(applyLearning({ kind: 'markUsed', id: 'learned:1' }, gone)).resolves.toBeNull();
  });
});

describe('learnedFormatSub', () => {
  const unicredit = { id: 'a', name: 'Běžný účet', institution: { name: 'UniCredit' } };
  const accounts = [
    unicredit,
    { id: 'b', name: 'Spořicí', institution: null },
    { id: 'c', name: 'Hotovost', institution: { name: '  ' } },
  ];

  it('names the account on its own for the row, without its institution', () => {
    expect(learnedFormatSub(format({ accountId: 'a' }), accounts).account).toBe('Běžný účet');
    expect(learnedFormatSub(format({ accountId: 'b' }), accounts).account).toBe('Spořicí');
  });

  it('names the account with its institution for the sheet', () => {
    expect(learnedFormatSub(format({ accountId: 'a' }), accounts).accountLabel).toBe(
      'Běžný účet · UniCredit'
    );
  });

  it('names an account without an institution by its name alone, blank or none', () => {
    expect(learnedFormatSub(format({ accountId: 'b' }), accounts).accountLabel).toBe('Spořicí');
    expect(learnedFormatSub(format({ accountId: 'c' }), accounts).accountLabel).toBe('Hotovost');
  });

  it('has no account when the account is gone or the format never had one', () => {
    for (const none of [
      learnedFormatSub(format({ accountId: 'gone' }), accounts),
      learnedFormatSub(format({ accountId: null }), accounts),
      learnedFormatSub(format({ accountId: 'a' }), []),
    ]) {
      expect(none.account).toBeNull();
      expect(none.accountLabel).toBeNull();
    }
  });

  it('passes the dates through as they are', () => {
    expect(
      learnedFormatSub(format({ createdAt: 1_760_000_000, lastUsedAt: 1_761_000_000 }), accounts)
    ).toEqual({
      account: null,
      accountLabel: null,
      created: 1_760_000_000,
      lastUsed: 1_761_000_000,
    });
  });
});
