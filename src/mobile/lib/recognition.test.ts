import { describe, expect, it } from 'vitest';
import type { CsvPreviewResult, InferredColumns, InferredField } from '@shared/schema';
import { EMPTY_MAPPING } from '@/components/bank-accounts/csv-import/import-config';
import {
  afterStartApply,
  applyOption,
  canImport,
  columnOptions,
  dayMonthOrder,
  looksLikeAmount,
  mappingFromInferred,
  maskedSample,
  parseSampleDate,
  recognitionStart,
  recognizedFields,
  selectedOption,
  signCheckFor,
  swapDayMonth,
  wasInferred,
} from './recognition';

const field = (column: string, confidence = 0.9): InferredField => ({ column, confidence });

const HEADERS = [
  'Datum zaúčtování',
  'Částka',
  'Zůstatek po pohybu',
  'Poplatek',
  'Název protiúčtu',
  'Číslo protiúčtu',
  'Zpráva pro příjemce',
  'Poznámka',
  'Měna',
  'ID operace',
];

const ROWS = [
  [
    '10.10.2026',
    '−1 249,00',
    '86 420,00',
    '0,00',
    'Lidl Praha 4',
    '123456789/0800',
    'Platba kartou',
    '',
    'CZK',
    '9001',
  ],
  [
    '09.10.2026',
    '61 200,00',
    '87 669,00',
    '0,00',
    'Umbrella a.s.',
    '987654321/0100',
    'Mzda 09/2026',
    'Září',
    'CZK',
    '9002',
  ],
  [
    '08.10.2026',
    '−189,00',
    '26 469,00',
    '0,00',
    'Bolt Operations',
    '',
    'Platba kartou',
    'Jízda',
    'CZK',
    '9003',
  ],
];

const inspection = (patch: Partial<CsvPreviewResult> = {}): CsvPreviewResult => ({
  headers: HEADERS,
  sampleRows: ROWS,
  totalRows: 48,
  delimiter: ';',
  suggestedMappings: {},
  headerRow: 0,
  detectedPresetId: null,
  dateFormat: '%d.%m.%Y',
  dateFormatAmbiguous: false,
  decimalSeparator: ',',
  encoding: 'UTF-8',
  suggestedDescriptionColumns: [],
  suggestedRowFilter: null,
  ...patch,
});

const inferred = (patch: Partial<InferredColumns> = {}): InferredColumns => ({
  date: field('Datum zaúčtování'),
  dateFormat: '%d.%m.%Y',
  dateAmbiguous: false,
  amount: field('Částka'),
  debit: null,
  credit: null,
  balance: field('Zůstatek po pohybu'),
  signCheck: 'verified',
  currency: field('Měna'),
  counterpartyIban: field('Číslo protiúčtu'),
  counterparty: field('Název protiúčtu'),
  transactionId: null,
  description: ['Zpráva pro příjemce', 'Poznámka'],
  decimalSeparator: ',',
  ...patch,
});

describe('mappingFromInferred', () => {
  it('takes the inferred columns, keeps the description columns in order and reads the decimals itself', () => {
    const mapping = mappingFromInferred(inferred(), inspection());
    expect(mapping).toMatchObject({
      dateColumn: 'Datum zaúčtování',
      dateFormat: '%d.%m.%Y',
      amountColumn: 'Částka',
      debitColumn: '',
      creditColumn: '',
      balanceColumn: 'Zůstatek po pohybu',
      currencyColumn: 'Měna',
      counterpartyColumn: 'Název protiúčtu',
      counterpartyIbanColumn: 'Číslo protiúčtu',
      transactionIdColumn: '',
      descriptionColumns: ['Zpráva pro příjemce', 'Poznámka'],
      // Another amount column picked later must not meet a stale separator
      decimalSeparator: 'auto',
      rowFilterEnabled: false,
    });
  });

  it('takes a debit and credit pair instead of an amount column', () => {
    const mapping = mappingFromInferred(
      inferred({ amount: null, debit: field('Poplatek'), credit: field('Částka') }),
      inspection()
    );
    expect(mapping.amountColumn).toBe('');
    expect(mapping.debitColumn).toBe('Poplatek');
    expect(mapping.creditColumn).toBe('Částka');
  });

  it('never half-fills a pair', () => {
    const mapping = mappingFromInferred(
      inferred({ amount: null, debit: field('Poplatek'), credit: null }),
      inspection()
    );
    expect(mapping.debitColumn).toBe('');
    expect(mapping.creditColumn).toBe('');
  });

  it('leaves what was not inferred empty and the date format to detect', () => {
    const mapping = mappingFromInferred(
      inferred({ date: null, dateFormat: null, amount: null, description: [] }),
      inspection()
    );
    expect(mapping.dateColumn).toBe('');
    expect(mapping.dateFormat).toBe('auto');
    expect(mapping.amountColumn).toBe('');
    expect(mapping.descriptionColumns).toEqual([]);
    expect(canImport(mapping)).toBe(false);
  });

  it('ignores a column the file has no header for', () => {
    const mapping = mappingFromInferred(
      inferred({ currency: field('Waehrung'), description: ['Zpráva pro příjemce', 'Nic'] }),
      inspection()
    );
    expect(mapping.currencyColumn).toBe('');
    expect(mapping.descriptionColumns).toEqual(['Zpráva pro příjemce']);
  });

  it('adds the bank code the headers name only next to an account number column', () => {
    const headers = [...HEADERS, 'Kód banky'];
    const withCode = inspection({
      headers,
      suggestedMappings: { counterpartyBankCode: ['Kód banky', 0.9] },
    });
    expect(mappingFromInferred(inferred(), withCode).counterpartyBankCodeColumn).toBe('Kód banky');
    expect(
      mappingFromInferred(inferred({ counterpartyIban: null }), withCode).counterpartyBankCodeColumn
    ).toBe('');
  });
});

describe('recognizedFields', () => {
  it('lists the fields in the order of the recognition with the columns each uses', () => {
    expect(recognizedFields(mappingFromInferred(inferred(), inspection()))).toEqual([
      { field: 'date', columns: ['Datum zaúčtování'] },
      { field: 'amount', columns: ['Částka'] },
      { field: 'description', columns: ['Zpráva pro příjemce', 'Poznámka'] },
      { field: 'counterparty', columns: ['Název protiúčtu', 'Číslo protiúčtu'] },
      { field: 'currency', columns: ['Měna'] },
      { field: 'balance', columns: ['Zůstatek po pohybu'] },
      { field: 'transactionId', columns: [] },
    ]);
  });

  it('names both columns of a pair and every field of an empty mapping', () => {
    const pair = recognizedFields({
      ...EMPTY_MAPPING,
      dateColumn: 'D',
      debitColumn: 'Out',
      creditColumn: 'In',
    });
    expect(pair.find((f) => f.field === 'amount')?.columns).toEqual(['Out', 'In']);
    expect(recognizedFields(EMPTY_MAPPING).map((f) => f.columns)).toEqual(
      Array(7).fill([] as string[])
    );
  });
});

describe('canImport', () => {
  it('needs a date and an amount or a whole pair', () => {
    expect(canImport({ ...EMPTY_MAPPING, dateColumn: 'D', amountColumn: 'A' })).toBe(true);
    expect(
      canImport({ ...EMPTY_MAPPING, dateColumn: 'D', debitColumn: 'O', creditColumn: 'I' })
    ).toBe(true);
    expect(canImport({ ...EMPTY_MAPPING, dateColumn: 'D', debitColumn: 'O' })).toBe(false);
    expect(canImport({ ...EMPTY_MAPPING, amountColumn: 'A' })).toBe(false);
    expect(canImport(EMPTY_MAPPING)).toBe(false);
  });
});

describe('recognitionStart', () => {
  const complete = mappingFromInferred(inferred(), inspection());

  it('opens the recognition when everything that matters was found', () => {
    expect(recognitionStart(complete, inferred())).toEqual({ stage: 'recognize' });
  });

  it('opens on the date correction when the date was not found', () => {
    const none = inferred({ date: null, dateFormat: null });
    expect(recognitionStart(mappingFromInferred(none, inspection()), none)).toEqual({
      stage: 'correct',
      field: 'date',
    });
  });

  it('opens on the amount correction when neither an amount nor a pair was found', () => {
    const none = inferred({ amount: null });
    expect(recognitionStart(mappingFromInferred(none, inspection()), none)).toEqual({
      stage: 'correct',
      field: 'amount',
    });
  });

  it('asks the date first when both are missing', () => {
    const none = inferred({ date: null, dateFormat: null, amount: null });
    expect(recognitionStart(mappingFromInferred(none, inspection()), none)).toEqual({
      stage: 'correct',
      field: 'date',
    });
  });

  it('counts a whole pair as the amount', () => {
    const pair = inferred({ amount: null, debit: field('Poplatek'), credit: field('Částka') });
    expect(recognitionStart(mappingFromInferred(pair, inspection()), pair)).toEqual({
      stage: 'recognize',
    });
  });

  it('asks the day and month order of an ambiguous date until it was confirmed', () => {
    const guess = inferred({ dateAmbiguous: true });
    const mapping = mappingFromInferred(guess, inspection());
    expect(recognitionStart(mapping, guess)).toEqual({ stage: 'correct', field: 'date' });
    expect(recognitionStart(mapping, guess, true)).toEqual({ stage: 'recognize' });
  });

  it('asks a missing amount before the order of the date', () => {
    const guess = inferred({ dateAmbiguous: true, amount: null });
    expect(recognitionStart(mappingFromInferred(guess, inspection()), guess)).toEqual({
      stage: 'correct',
      field: 'amount',
    });
  });

  it('does not ask the order of a date column the person chose themselves', () => {
    const guess = inferred({ dateAmbiguous: true });
    const mapping = { ...mappingFromInferred(guess, inspection()), dateColumn: 'Poznámka' };
    expect(recognitionStart(mapping, guess)).toEqual({ stage: 'recognize' });
  });
});

describe('afterStartApply', () => {
  it('ends unreadable when the date or the amount is still missing after the correction', () => {
    const none = inferred({ date: null, dateFormat: null, amount: null });
    const base = mappingFromInferred(none, inspection());
    expect(afterStartApply({ ...base, dateColumn: 'Datum zaúčtování' }, none, true)).toEqual({
      stage: 'unreadable',
    });
    expect(afterStartApply({ ...base, amountColumn: 'Částka' }, none, false)).toEqual({
      stage: 'unreadable',
    });
  });

  it('goes on to the recognition, or to the order of the date still to be asked', () => {
    const guess = inferred({ dateAmbiguous: true, amount: null });
    const base = mappingFromInferred(guess, inspection());
    expect(afterStartApply({ ...base, amountColumn: 'Částka' }, guess, false)).toEqual({
      stage: 'correct',
      field: 'date',
    });
    expect(afterStartApply({ ...base, amountColumn: 'Částka' }, guess, true)).toEqual({
      stage: 'recognize',
    });
  });
});

describe('columnOptions', () => {
  const options = (name: Parameters<typeof columnOptions>[0], patch?: Partial<InferredColumns>) =>
    columnOptions(name, inspection(), inferred(patch));
  const ids = (list: ReturnType<typeof columnOptions>) => list.map((o) => o.id);

  it('lists every column with its header and two sample values, in file order', () => {
    const list = options('counterparty');
    const columns = list.filter((o) => o.kind === 'column');
    expect(columns.map((o) => o.label)).toEqual(HEADERS);
    const name = columns.find((o) => o.label === 'Název protiúčtu');
    expect(name?.samples).toEqual(['Lidl Praha 4', 'Umbrella a.s.']);
    // An empty cell is no sample: the next row stands in
    const note = columns.find((o) => o.label === 'Poznámka');
    expect(note?.samples).toEqual(['Září', 'Jízda']);
    expect(list.every((o) => o.samples.length <= 2)).toBe(true);
  });

  it('skips a header that is blank or repeated', () => {
    const list = columnOptions(
      'currency',
      inspection({ headers: ['A', '', 'A', 'B'], sampleRows: [['1', '2', '3', '4']] }),
      inferred()
    );
    expect(list.filter((o) => o.kind === 'column').map((o) => o.label)).toEqual(['A', 'B']);
  });

  it('offers "nepoužít" first, and only for a field that is optional', () => {
    for (const optional of [
      'description',
      'counterparty',
      'currency',
      'balance',
      'transactionId',
    ] as const) {
      expect(options(optional)[0]).toMatchObject({ id: 'none', kind: 'none', samples: [] });
    }
    for (const required of ['date', 'amount'] as const) {
      expect(ids(options(required))).not.toContain('none');
    }
  });

  it('clears the columns of the field with "nepoužít"', () => {
    expect(options('currency')[0].apply).toEqual({ currencyColumn: '' });
    expect(options('balance')[0].apply).toEqual({ balanceColumn: '' });
    expect(options('transactionId')[0].apply).toEqual({ transactionIdColumn: '' });
    expect(options('description')[0].apply).toEqual({ descriptionColumns: [] });
    expect(options('counterparty')[0].apply).toEqual({
      counterpartyColumn: '',
      counterpartyIbanColumn: '',
    });
  });

  it('applies one column to the field', () => {
    const apply = (name: Parameters<typeof columnOptions>[0], header: string) =>
      options(name).find((o) => o.id === `col:${header}`)?.apply;
    expect(apply('currency', 'Měna')).toEqual({ currencyColumn: 'Měna' });
    expect(apply('counterparty', 'Název protiúčtu')).toEqual({
      counterpartyColumn: 'Název protiúčtu',
    });
    expect(apply('description', 'Poznámka')).toEqual({ descriptionColumns: ['Poznámka'] });
    expect(apply('balance', 'Zůstatek po pohybu')).toEqual({ balanceColumn: 'Zůstatek po pohybu' });
  });

  it('offers all the description columns together first when there are several', () => {
    const list = options('description');
    const together = list.find((o) => o.kind === 'columns');
    expect(together?.label).toBe('Zpráva pro příjemce + Poznámka');
    expect(together?.apply).toEqual({ descriptionColumns: ['Zpráva pro příjemce', 'Poznámka'] });
    expect(together?.samples).toEqual(['Platba kartou', 'Mzda 09/2026 · Září']);
    expect(
      options('description', { description: ['Poznámka'] }).some((o) => o.kind === 'columns')
    ).toBe(false);
  });

  describe('amount', () => {
    it('applies a single column and clears a pair', () => {
      const single = options('amount').find((o) => o.id === 'col:Částka');
      expect(single?.apply).toEqual({ amountColumn: 'Částka', debitColumn: '', creditColumn: '' });
      expect(single?.samples).toEqual(['−1 249,00', '61 200,00']);
    });

    it('offers the inferred debit and credit pair first, with both cells in the samples', () => {
      const pair = inferred({
        amount: null,
        debit: field('Poplatek'),
        credit: field('Částka'),
      });
      const list = columnOptions('amount', inspection(), pair);
      expect(list[0]).toMatchObject({
        id: 'pair:Poplatek|Částka',
        kind: 'pair',
        label: 'Poplatek + Částka',
        apply: { amountColumn: '', debitColumn: 'Poplatek', creditColumn: 'Částka' },
        samples: ['0,00 / −1 249,00', '0,00 / 61 200,00'],
      });
    });

    it('marks the cells of every option as amounts, so they can be masked', () => {
      expect(options('amount').every((o) => o.money)).toBe(true);
      // "nepoužít" has no cells
      expect(
        options('balance')
          .filter((o) => o.kind !== 'none')
          .every((o) => o.money)
      ).toBe(true);
    });

    it('finds a pair the inference left out: one side filled per row, left is the debit', () => {
      const paidOut = inspection({
        headers: ['Date', 'Details', 'Paid out', 'Paid in'],
        sampleRows: [
          ['01/02/2026', 'Tesco', '12.50', ''],
          ['02/02/2026', 'Salary', '', '1500.00'],
          ['03/02/2026', 'Coffee', '3.20', ''],
        ],
      });
      const list = columnOptions('amount', paidOut, inferred({ amount: null }));
      const pair = list.find((o) => o.kind === 'pair');
      expect(pair?.apply).toEqual({
        amountColumn: '',
        debitColumn: 'Paid out',
        creditColumn: 'Paid in',
      });
      expect(pair?.samples).toEqual(['12.50 / —', '— / 1500.00']);
    });

    it('lets a header that names the sides orient a pair the other way round', () => {
      const swapped = inspection({
        headers: ['Date', 'In', 'Out'],
        sampleRows: [
          ['01/02/2026', '', '12.50'],
          ['02/02/2026', '1500.00', ''],
        ],
        suggestedMappings: { debit: ['Out', 0.9], credit: ['In', 0.9] },
      });
      const pair = columnOptions('amount', swapped, inferred({ amount: null })).find(
        (o) => o.kind === 'pair'
      );
      expect(pair?.apply).toMatchObject({ debitColumn: 'Out', creditColumn: 'In' });
    });

    it('does not offer two columns that are both filled on one row, or that never are', () => {
      const both = inspection({
        headers: ['Date', 'A', 'B'],
        sampleRows: [
          ['01/02/2026', '1.00', '2.00'],
          ['02/02/2026', '3.00', ''],
        ],
      });
      expect(
        columnOptions('amount', both, inferred({ amount: null })).some((o) => o.kind === 'pair')
      ).toBe(false);
      const never = inspection({
        headers: ['Date', 'A', 'B'],
        sampleRows: [
          ['01/02/2026', '1.00', ''],
          ['02/02/2026', '3.00', ''],
        ],
      });
      expect(
        columnOptions('amount', never, inferred({ amount: null })).some((o) => o.kind === 'pair')
      ).toBe(false);
    });
  });

  describe('date', () => {
    it('offers every column and keeps the inferred format only for the inferred column', () => {
      const list = options('date');
      expect(list.filter((o) => o.kind === 'column')).toHaveLength(HEADERS.length);
      expect(list.find((o) => o.id === 'col:Datum zaúčtování')?.apply).toEqual({
        dateColumn: 'Datum zaúčtování',
        dateFormat: '%d.%m.%Y',
      });
      // Another column's format is not known: the import detects it
      expect(list.find((o) => o.id === 'col:Poznámka')?.apply).toEqual({
        dateColumn: 'Poznámka',
        dateFormat: 'auto',
      });
    });

    it('offers all the columns, none checked, when no date was found', () => {
      const none = inferred({ date: null, dateFormat: null });
      const list = columnOptions('date', inspection(), none);
      expect(list).toHaveLength(HEADERS.length);
      expect(selectedOption(list, mappingFromInferred(none, inspection()))).toBeNull();
    });

    describe('ambiguous', () => {
      const dates = inspection({
        sampleRows: [
          ['03.04.2026', ...ROWS[0].slice(1)],
          ['05.06.2026', ...ROWS[1].slice(1)],
          ['06.06.2026', ...ROWS[2].slice(1)],
        ],
      });
      const guess = inferred({ dateAmbiguous: true });
      const list = columnOptions('date', dates, guess);

      it('puts the two orders of the date column in its place, one sample date for each', () => {
        expect(list.map((o) => o.id).slice(0, 3)).toEqual(['order:dmy', 'order:mdy', 'col:Částka']);
        const [dayFirst, monthFirst] = list;
        expect(dayFirst.apply).toEqual({
          dateColumn: 'Datum zaúčtování',
          dateFormat: '%d.%m.%Y',
        });
        expect(monthFirst.apply).toEqual({
          dateColumn: 'Datum zaúčtování',
          dateFormat: '%m.%d.%Y',
        });
        // 3 April and 4 March, as unix seconds of the UTC day
        expect(dayFirst.dates).toEqual([Date.UTC(2026, 3, 3) / 1000]);
        expect(monthFirst.dates).toEqual([Date.UTC(2026, 2, 4) / 1000]);
        expect(dayFirst.label).toBe('Datum zaúčtování');
        expect(dayFirst.order).toBe('dmy');
        expect(monthFirst.order).toBe('mdy');
      });

      it('shows a date whose day and month differ, so the two readings can be told apart', () => {
        const same = inspection({
          sampleRows: [
            ['04.04.2026', ...ROWS[0].slice(1)],
            ['05.06.2026', ...ROWS[1].slice(1)],
          ],
        });
        const [dayFirst, monthFirst] = columnOptions('date', same, guess);
        expect(dayFirst.dates).toEqual([Date.UTC(2026, 5, 5) / 1000]);
        expect(monthFirst.dates).toEqual([Date.UTC(2026, 4, 6) / 1000]);
      });

      it('selects the guess until the person picks the other order', () => {
        const mapping = mappingFromInferred(guess, dates);
        expect(selectedOption(list, mapping)).toBe('order:dmy');
        const picked = applyOption(mapping, list[1]);
        expect(selectedOption(list, picked)).toBe('order:mdy');
        expect(picked.dateFormat).toBe('%m.%d.%Y');
      });

      it('keeps the other columns as columns', () => {
        expect(list.find((o) => o.id === 'col:Poznámka')?.apply).toEqual({
          dateColumn: 'Poznámka',
          dateFormat: 'auto',
        });
      });

      it('has no samples when the cell cannot be read in the guessed format', () => {
        const odd = inspection({ sampleRows: [['soon', ...ROWS[0].slice(1)]] });
        expect(columnOptions('date', odd, guess)[0].dates).toEqual([]);
      });
    });
  });
});

describe('selectedOption and applyOption', () => {
  it('selects the option the mapping already has', () => {
    const mapping = mappingFromInferred(inferred(), inspection());
    expect(selectedOption(columnOptions('amount', inspection(), inferred()), mapping)).toBe(
      'col:Částka'
    );
    expect(selectedOption(columnOptions('currency', inspection(), inferred()), mapping)).toBe(
      'col:Měna'
    );
    expect(selectedOption(columnOptions('transactionId', inspection(), inferred()), mapping)).toBe(
      'none'
    );
    expect(selectedOption(columnOptions('description', inspection(), inferred()), mapping)).toBe(
      'cols:Zpráva pro příjemce|Poznámka'
    );
  });

  it('applies an option over the mapping without touching the rest', () => {
    const mapping = mappingFromInferred(inferred(), inspection());
    const list = columnOptions('amount', inspection(), inferred());
    const next = applyOption(
      mapping,
      list.find((o) => o.id === 'col:Zůstatek po pohybu')!
    );
    expect(next.amountColumn).toBe('Zůstatek po pohybu');
    expect(next.dateColumn).toBe(mapping.dateColumn);
    expect(next.descriptionColumns).toBe(mapping.descriptionColumns);
  });
});

describe('what the recognition says about the amount', () => {
  it('passes the sign check on only while the amount is the one it checked', () => {
    const verified = inferred({ signCheck: 'verified' });
    const mapping = mappingFromInferred(verified, inspection());
    expect(signCheckFor(mapping, verified)).toBe('verified');
    expect(signCheckFor({ ...mapping, amountColumn: 'Poplatek' }, verified)).toBe('unchecked');
    expect(signCheckFor(mapping, inferred({ signCheck: 'inverted' }))).toBe('inverted');
    expect(signCheckFor(mapping, inferred({ signCheck: 'unchecked' }))).toBe('unchecked');
  });

  it('knows a pair was the thing inferred', () => {
    const pair = inferred({
      amount: null,
      debit: field('Poplatek'),
      credit: field('Částka'),
      signCheck: 'verified',
    });
    expect(signCheckFor(mappingFromInferred(pair, inspection()), pair)).toBe('verified');
  });
});

describe('wasInferred', () => {
  it('tells "not found" from "left out by the person"', () => {
    const found = inferred({ transactionId: null, currency: null });
    expect(wasInferred('date', found)).toBe(true);
    expect(wasInferred('amount', found)).toBe(true);
    expect(wasInferred('balance', found)).toBe(true);
    expect(wasInferred('transactionId', found)).toBe(false);
    expect(wasInferred('currency', found)).toBe(false);
    expect(wasInferred('amount', inferred({ amount: null }))).toBe(false);
    expect(
      wasInferred('amount', inferred({ amount: null, debit: field('A'), credit: field('B') }))
    ).toBe(true);
    expect(wasInferred('counterparty', inferred({ counterparty: null }))).toBe(true);
    expect(wasInferred('description', inferred({ description: [] }))).toBe(false);
  });
});

describe('the day and month order', () => {
  it('swaps day and month in a chrono format and nothing else', () => {
    expect(swapDayMonth('%d.%m.%Y')).toBe('%m.%d.%Y');
    expect(swapDayMonth('%m/%d/%y')).toBe('%d/%m/%y');
    expect(swapDayMonth('%Y-%m-%d')).toBe('%Y-%d-%m');
    expect(swapDayMonth('%d %b %Y')).toBe('%m %b %Y');
  });

  it('names the order by which of day and month comes first', () => {
    expect(dayMonthOrder('%d.%m.%Y')).toBe('dmy');
    expect(dayMonthOrder('%m/%d/%Y')).toBe('mdy');
    expect(dayMonthOrder('%Y-%m-%d')).toBe('mdy');
    expect(dayMonthOrder('%Y年%m月%d日')).toBe('mdy');
    expect(dayMonthOrder('%d %b %Y')).toBeNull();
    expect(dayMonthOrder('auto')).toBeNull();
  });

  it('reads a sample cell in a numeric format, with a time after it or without', () => {
    const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;
    expect(parseSampleDate('03.04.2026', '%d.%m.%Y')).toBe(day(2026, 4, 3));
    expect(parseSampleDate(' 03.04.2026 12:30:00', '%d.%m.%Y')).toBe(day(2026, 4, 3));
    expect(parseSampleDate('03.04.2026', '%m.%d.%Y')).toBe(day(2026, 3, 4));
    expect(parseSampleDate('2026-04-03T08:00', '%Y-%m-%d')).toBe(day(2026, 4, 3));
    expect(parseSampleDate('3/4/26', '%d/%m/%y')).toBe(day(2026, 4, 3));
    expect(parseSampleDate('3/4/99', '%d/%m/%y')).toBe(day(1999, 4, 3));
  });

  it('refuses what is no date', () => {
    expect(parseSampleDate('31.02.2026', '%d.%m.%Y')).toBeNull();
    expect(parseSampleDate('13.13.2026', '%d.%m.%Y')).toBeNull();
    expect(parseSampleDate('soon', '%d.%m.%Y')).toBeNull();
    expect(parseSampleDate('03.04.2026', '%d %b %Y')).toBeNull();
    expect(parseSampleDate('', '%d.%m.%Y')).toBeNull();
  });
});

describe('masking the sample cells', () => {
  it('recognizes a cell that looks like an amount, and not a date or an account', () => {
    for (const amount of [
      '1\u00a0249,00',
      '−1\u202f249,00 Kč',
      '−1 249,00',
      '61 200,00',
      '-189,00',
      '1,249.50',
      '12.50',
      '0,00',
      '(12,00)',
    ]) {
      expect(looksLikeAmount(amount), amount).toBe(true);
    }
    for (const other of [
      '10.10.2026',
      '10.10.26',
      '2026-10-10',
      'Platba kartou',
      '123456789/0800',
      'CZ65 0800 0000 1920 0014 5399',
      '9001',
      '',
    ]) {
      expect(looksLikeAmount(other), other).toBe(false);
    }
  });

  it('masks the digits of a cell of an amount column, whatever it looks like', () => {
    expect(maskedSample('2000', { money: true })).toBe('••••');
    expect(maskedSample('−1 249,00 Kč', { money: true })).toBe('−•••• Kč');
    expect(maskedSample('Platba kartou', { money: true })).toBe('Platba kartou');
  });

  it('masks only what looks like an amount in any other column', () => {
    expect(maskedSample('61 200,00', { money: false })).toBe('••••');
    expect(maskedSample('10.10.2026', { money: false })).toBe('10.10.2026');
    expect(maskedSample('Lidl Praha 4', { money: false })).toBe('Lidl Praha 4');
  });

  it('marks the columns that hold amounts, wherever they are listed', () => {
    const list = columnOptions('counterparty', inspection(), inferred());
    const money = list.filter((o) => o.money).map((o) => o.label);
    expect(money).toEqual(['Částka', 'Zůstatek po pohybu']);
  });
});
