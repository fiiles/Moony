import { describe, expect, it } from 'vitest';
import type { CsvPreviewResult, InferredColumns, InferredField } from '@shared/schema';
import { EMPTY_MAPPING } from '@/components/bank-accounts/csv-import/import-config';
import {
  afterStartApply,
  ambiguousDateFormat,
  amountsAllPositive,
  applyOption,
  canImport,
  columnOptions,
  dateOrderOf,
  dayMonthOrder,
  mappingFromInferred,
  parseSampleDate,
  recognitionStart,
  recognizedFields,
  selectedOption,
  signCheckFor,
  signsBlockImport,
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
  const noDateNoAmount = inferred({ date: null, dateFormat: null, amount: null });
  const none = mappingFromInferred(noDateNoAmount, inspection());

  it('asks the amount after the date when both were missing, then shows the rows', () => {
    // The recognition opens on the date ...
    expect(recognitionStart(none, noDateNoAmount)).toEqual({ stage: 'correct', field: 'date' });
    // ... the person answers it: the file is no statement only if they leave, so the amount is next
    const dated = { ...none, dateColumn: 'Datum zaúčtování' };
    expect(afterStartApply(dated, noDateNoAmount, true, 'date')).toEqual({
      stage: 'correct',
      field: 'amount',
    });
    // ... and the amount answered, nothing is missing
    const both = { ...dated, amountColumn: 'Částka' };
    expect(afterStartApply(both, noDateNoAmount, true, 'amount')).toEqual({ stage: 'recognize' });
  });

  it('asks the date after the amount when that is the one answered first', () => {
    expect(
      afterStartApply({ ...none, amountColumn: 'Částka' }, noDateNoAmount, false, 'amount')
    ).toEqual({ stage: 'correct', field: 'date' });
  });

  it('ends unreadable only for a required field that is still empty after its own question', () => {
    expect(afterStartApply(none, noDateNoAmount, true, 'date')).toEqual({ stage: 'unreadable' });
    expect(
      afterStartApply({ ...none, dateColumn: 'Datum zaúčtování' }, noDateNoAmount, true, 'amount')
    ).toEqual({ stage: 'unreadable' });
  });

  it('goes on to the recognition, or to the order of the date still to be asked', () => {
    const guess = inferred({ dateAmbiguous: true, amount: null });
    const base = mappingFromInferred(guess, inspection());
    expect(afterStartApply({ ...base, amountColumn: 'Částka' }, guess, false, 'amount')).toEqual({
      stage: 'correct',
      field: 'date',
    });
    expect(afterStartApply({ ...base, amountColumn: 'Částka' }, guess, true, 'amount')).toEqual({
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
        // The sides are named: which column is read as the money out
        sides: { debit: 'Poplatek', credit: 'Částka' },
        apply: { amountColumn: '', debitColumn: 'Poplatek', creditColumn: 'Částka' },
        samples: ['0,00 / −1 249,00', '0,00 / 61 200,00'],
      });
      // The inference oriented it (a balance, the signs or the headers): it is offered one way only
      expect(list.filter((o) => o.kind === 'pair')).toHaveLength(1);
    });

    it('offers a pair the inference left out both ways round when nothing tells its sides', () => {
      const paidOut = inspection({
        headers: ['Date', 'Details', 'Paid out', 'Paid in'],
        sampleRows: [
          ['01/02/2026', 'Tesco', '12.50', ''],
          ['02/02/2026', 'Salary', '', '1500.00'],
          ['03/02/2026', 'Coffee', '3.20', ''],
        ],
      });
      const list = columnOptions('amount', paidOut, inferred({ amount: null }));
      const pairs = list.filter((o) => o.kind === 'pair');
      expect(pairs.map((o) => o.id)).toEqual(['pair:Paid out|Paid in', 'pair:Paid in|Paid out']);
      const [leftOut, rightOut] = pairs;
      expect(leftOut.sides).toEqual({ debit: 'Paid out', credit: 'Paid in' });
      expect(leftOut.apply).toEqual({
        amountColumn: '',
        debitColumn: 'Paid out',
        creditColumn: 'Paid in',
      });
      expect(leftOut.samples).toEqual(['12.50 / —', '— / 1500.00']);
      // The other way round: the money out is the right column, and the samples follow the sides
      expect(rightOut.sides).toEqual({ debit: 'Paid in', credit: 'Paid out' });
      expect(rightOut.apply).toEqual({
        amountColumn: '',
        debitColumn: 'Paid in',
        creditColumn: 'Paid out',
      });
      expect(rightOut.samples).toEqual(['— / 12.50', '1500.00 / —']);
      // Each is its own answer
      const mapping = mappingFromInferred(inferred({ amount: null }), paidOut);
      expect(selectedOption(list, applyOption(mapping, rightOut))).toBe('pair:Paid in|Paid out');
      expect(selectedOption(list, applyOption(mapping, leftOut))).toBe('pair:Paid out|Paid in');
    });

    it('lets a header that names the sides orient a pair, offered that way only', () => {
      const swapped = inspection({
        headers: ['Date', 'In', 'Out'],
        sampleRows: [
          ['01/02/2026', '', '12.50'],
          ['02/02/2026', '1500.00', ''],
        ],
        suggestedMappings: { debit: ['Out', 0.9], credit: ['In', 0.9] },
      });
      const pairs = columnOptions('amount', swapped, inferred({ amount: null })).filter(
        (o) => o.kind === 'pair'
      );
      expect(pairs).toHaveLength(1);
      expect(pairs[0].apply).toMatchObject({ debitColumn: 'Out', creditColumn: 'In' });
      expect(pairs[0].sides).toEqual({ debit: 'Out', credit: 'In' });
      // One side named is enough, as in the inference
      const creditOnly = inspection({
        headers: ['Date', 'In', 'Out'],
        sampleRows: swapped.sampleRows,
        suggestedMappings: { credit: ['In', 0.9] },
      });
      const oriented = columnOptions('amount', creditOnly, inferred({ amount: null })).filter(
        (o) => o.kind === 'pair'
      );
      expect(oriented.map((o) => o.id)).toEqual(['pair:Out|In']);
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
      // The fixture's dates (10.10., 9.10., 8.10.) read both ways: the column comes twice, once per order
      expect(list.filter((o) => o.kind === 'column')).toHaveLength(HEADERS.length - 1);
      expect(list.filter((o) => o.kind === 'order').map((o) => o.id)).toEqual([
        'order:Datum zaúčtování:dmy',
        'order:Datum zaúčtování:mdy',
      ]);
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
        expect(list.map((o) => o.id).slice(0, 3)).toEqual([
          'order:Datum zaúčtování:dmy',
          'order:Datum zaúčtování:mdy',
          'col:Částka',
        ]);
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
        expect(selectedOption(list, mapping)).toBe('order:Datum zaúčtování:dmy');
        const picked = applyOption(mapping, list[1]);
        expect(selectedOption(list, picked)).toBe('order:Datum zaúčtování:mdy');
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

    describe('a column the person may choose', () => {
      const withValueDate = (cells: string[]) =>
        inspection({
          headers: ['Datum zaúčtování', 'Datum valuty', 'Částka'],
          sampleRows: cells.map((cell, i) => ['25.04.2026', cell, `${i + 1},00`]),
        });
      const settled = inferred({ dateAmbiguous: false });

      it('asks the order of another column whose dates read both ways, as for the inferred one', () => {
        const list = columnOptions('date', withValueDate(['03.04.2026', '05.06.2026']), settled);
        expect(list.map((o) => o.id)).toEqual([
          'col:Datum zaúčtování',
          'order:Datum valuty:dmy',
          'order:Datum valuty:mdy',
          'col:Částka',
        ]);
        const [, dayFirst, monthFirst] = list;
        expect(dayFirst.apply).toEqual({ dateColumn: 'Datum valuty', dateFormat: '%d.%m.%Y' });
        expect(monthFirst.apply).toEqual({ dateColumn: 'Datum valuty', dateFormat: '%m.%d.%Y' });
        expect(dayFirst.dates).toEqual([Date.UTC(2026, 3, 3) / 1000]);
        expect(monthFirst.dates).toEqual([Date.UTC(2026, 2, 4) / 1000]);
        // Choosing one of them is the answer: nothing is left to the import's own guess
        const mapping = mappingFromInferred(settled, withValueDate(['03.04.2026']));
        const picked = applyOption(mapping, monthFirst);
        expect(picked.dateFormat).toBe('%m.%d.%Y');
        expect(selectedOption(list, picked)).toBe('order:Datum valuty:mdy');
      });

      it('leaves a column a value above 12 decides to the import', () => {
        const list = columnOptions('date', withValueDate(['03.04.2026', '25.06.2026']), settled);
        expect(list.find((o) => o.id === 'col:Datum valuty')?.apply).toEqual({
          dateColumn: 'Datum valuty',
          dateFormat: 'auto',
        });
        expect(list.some((o) => o.kind === 'order')).toBe(false);
      });

      it('takes the inference at its word for its own column, whatever the sample rows say', () => {
        const rows = inspection({
          headers: ['Datum', 'Částka'],
          sampleRows: [['03.04.2026', '1,00']],
        });
        const own = inferred({ date: field('Datum'), dateAmbiguous: false });
        expect(columnOptions('date', rows, own).some((o) => o.kind === 'order')).toBe(false);
      });

      it('shows the order the person chose next to the column', () => {
        const mapping = {
          ...mappingFromInferred(settled, withValueDate(['03.04.2026'])),
          dateColumn: 'Datum valuty',
          dateFormat: '%m.%d.%Y',
        };
        expect(dateOrderOf(mapping, settled)).toBe('mdy');
        // A column left to the import, and the inferred column that was never a question, show none
        expect(dateOrderOf({ ...mapping, dateFormat: 'auto' }, settled)).toBeNull();
        expect(dateOrderOf(mappingFromInferred(settled, inspection()), settled)).toBeNull();
        const guess = inferred({ dateAmbiguous: true });
        expect(dateOrderOf(mappingFromInferred(guess, inspection()), guess)).toBe('dmy');
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

  it('blocks the import while the balance proves the expenses positive, until another column is chosen', () => {
    // A card statement printing expenses positive: the import cannot flip the signs
    const inverted = inferred({ signCheck: 'inverted' });
    const mapping = mappingFromInferred(inverted, inspection());
    expect(signsBlockImport(mapping, inverted)).toBe(true);
    expect(signsBlockImport({ ...mapping, amountColumn: 'Poplatek' }, inverted)).toBe(false);
    expect(
      signsBlockImport(
        { ...mapping, amountColumn: '', debitColumn: 'Poplatek', creditColumn: 'Částka' },
        inverted
      )
    ).toBe(false);
    for (const signCheck of ['verified', 'unchecked'] as const) {
      const other = inferred({ signCheck });
      expect(signsBlockImport(mappingFromInferred(other, inspection()), other)).toBe(false);
    }
  });
});

describe('amountsAllPositive', () => {
  const file = (cells: string[]) =>
    inspection({
      headers: ['Datum', 'Popis', 'Kolik', 'Částka'],
      sampleRows: cells.map((cell, i) => [`2${i}.09.2026`, 'Nákup', cell, `-${i + 1},00`]),
    });
  const read = inferred({
    date: field('Datum'),
    amount: field('Částka'),
    balance: null,
    signCheck: 'unchecked',
  });
  const chosen = (cells: string[], column = 'Kolik') => ({
    ...mappingFromInferred(read, file(cells)),
    amountColumn: column,
  });

  it('warns when the chosen single column holds only positive amounts and nothing proves it', () => {
    for (const cells of [
      ['1500', '25000', '189'],
      ['1 249,00', '+61,00', '0,00'],
      ['12,50 €', 'CZK 5 000', ''],
    ]) {
      expect(amountsAllPositive(chosen(cells), read, file(cells)), cells.join(' | ')).toBe(true);
    }
  });

  it('says nothing of a column with a negative amount, however the minus is written', () => {
    for (const cell of ['-189', '−189,00', '(12,00)', '12,00-', '-€ 12,50', 'EUR -5,00']) {
      const cells = ['1500', cell];
      expect(amountsAllPositive(chosen(cells), read, file(cells)), cell).toBe(false);
    }
  });

  it('says nothing of a column that is no amount, empty or all zero', () => {
    for (const cells of [
      ['Platba', '1500'],
      ['', ''],
      ['0,00', '0'],
    ]) {
      expect(amountsAllPositive(chosen(cells), read, file(cells)), cells.join(' | ')).toBe(false);
    }
  });

  it('says nothing of the inferred column, a pair or a column whose signs a balance proved', () => {
    const cells = ['1500', '25000'];
    // The inference read its own column over many more rows: unproven, it has both signs
    const own = inspection({
      headers: ['Datum', 'Částka'],
      sampleRows: [
        ['20.09.2026', '1500'],
        ['21.09.2026', '25000'],
      ],
    });
    expect(amountsAllPositive(mappingFromInferred(read, own), read, own)).toBe(false);
    expect(
      amountsAllPositive(
        { ...chosen(cells), amountColumn: '', debitColumn: 'Kolik', creditColumn: 'Částka' },
        read,
        file(cells)
      )
    ).toBe(false);
    const verified = inferred({ amount: field('Kolik'), signCheck: 'verified' });
    expect(amountsAllPositive(chosen(cells), verified, file(cells))).toBe(false);
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

describe('ambiguousDateFormat', () => {
  it('names the day-first format of a column whose dates all read both ways', () => {
    expect(ambiguousDateFormat(['03.04.2026', '05.06.2026'])).toBe('%d.%m.%Y');
    expect(ambiguousDateFormat(['3/4/26', '', '5/6/26'])).toBe('%d/%m/%y');
    expect(ambiguousDateFormat(['03-04-2026 12:30', '05-06-2026'])).toBe('%d-%m-%Y');
    expect(ambiguousDateFormat(['03. 04. 2026', '05. 06. 2026'])).toBe('%d. %m. %Y');
  });

  it('has nothing to ask when a value decides, the cells disagree or it is no date', () => {
    expect(ambiguousDateFormat(['03.04.2026', '25.06.2026'])).toBeNull();
    expect(ambiguousDateFormat(['03.04.2026', '05.13.2026'])).toBeNull();
    expect(ambiguousDateFormat(['03.04.2026', '05/06/2026'])).toBeNull();
    expect(ambiguousDateFormat(['03.04.2026', '05.06.26'])).toBeNull();
    expect(ambiguousDateFormat(['2026-04-03'])).toBeNull();
    expect(ambiguousDateFormat(['Platba kartou'])).toBeNull();
    expect(ambiguousDateFormat(['1,50', '2,00'])).toBeNull();
    expect(ambiguousDateFormat(['', ' '])).toBeNull();
    expect(ambiguousDateFormat([])).toBeNull();
  });
});
