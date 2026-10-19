/**
 * The guided recognition of an unknown CSV (spec §8.6 rung 3), pure. The inference
 * (`inferCsvColumns`) names the columns of a file no preset describes; this module turns its answer
 * into the desktop's `MappingState`, says what the phone shows of it ("Takhle tomu rozumím") and
 * builds the per-field correction: a list of the file's columns, each with its header and two
 * sample values, never a mapping grid. The date and the amount are never guessed silently: when
 * either is missing, or the day and month of the dates cannot be told, the recognition opens on
 * that field's correction (`recognitionStart`).
 */
import type { CsvPreviewResult, InferredColumns, InferredField, SignCheck } from '@shared/schema';
import {
  AUTO_VALUE,
  EMPTY_MAPPING,
  isMappingComplete,
  mappingFromInspection,
  uniqueHeaders,
  usesDebitCredit,
  type MappingState,
} from '@/components/bank-accounts/csv-import/import-config';
import { maskMoney } from '@/mobile/lib/hide-amounts';

/** The fields the recognition names, in the order it lists them. */
export const RECOGNITION_FIELDS = [
  'date',
  'amount',
  'description',
  'counterparty',
  'currency',
  'balance',
  'transactionId',
] as const;

export type RecognitionField = (typeof RECOGNITION_FIELDS)[number];

/** Without these two a file is no statement the app can read. */
export function isRequiredField(field: RecognitionField): boolean {
  return field === 'date' || field === 'amount';
}

/** Rows of the file shown as Moony rows under "Takhle tomu rozumím". */
export const SAMPLE_ROWS = 3;

/** Sample values shown under a column's header in the correction. */
const SAMPLES_PER_COLUMN = 2;

/** The columns the content inference could name, `''` for the ones it left out. */
export function mappingFromInferred(
  inferred: InferredColumns,
  inspection: CsvPreviewResult
): MappingState {
  const headers = new Set(uniqueHeaders(inspection.headers));
  const column = (found: InferredField | null): string =>
    found !== null && headers.has(found.column) ? found.column : '';

  const date = column(inferred.date);
  const amount = column(inferred.amount);
  const debit = column(inferred.debit);
  const credit = column(inferred.credit);
  const pair = amount === '' && debit !== '' && credit !== '';
  const account = column(inferred.counterpartyIban);
  // The content knows nothing of a bank code; the headers' own names decide it, next to an account number
  const bankCode = mappingFromInspection(inspection).counterpartyBankCodeColumn;

  return {
    ...EMPTY_MAPPING,
    dateColumn: date,
    dateFormat: date !== '' ? (inferred.dateFormat ?? AUTO_VALUE) : AUTO_VALUE,
    amountColumn: amount,
    debitColumn: pair ? debit : '',
    creditColumn: pair ? credit : '',
    balanceColumn: column(inferred.balance),
    currencyColumn: column(inferred.currency),
    counterpartyColumn: column(inferred.counterparty),
    counterpartyIbanColumn: account,
    counterpartyBankCodeColumn: account !== '' && headers.has(bankCode) ? bankCode : '',
    transactionIdColumn: column(inferred.transactionId),
    descriptionColumns: inferred.description.filter((name) => headers.has(name)),
    // Left to detect, so another amount column picked later is read with its own separator
    decimalSeparator: AUTO_VALUE,
    rowFilterEnabled: false,
  };
}

/** A date and either one amount column or a whole debit and credit pair. */
export function canImport(mapping: MappingState): boolean {
  return isMappingComplete(mapping);
}

/** The fields in the order of the recognition, with the columns each reads (none = not used). */
export function recognizedFields(
  mapping: MappingState
): Array<{ field: RecognitionField; columns: string[] }> {
  const used = (...names: string[]) => names.filter((name) => name !== '');
  return [
    { field: 'date', columns: used(mapping.dateColumn) },
    {
      field: 'amount',
      columns: usesDebitCredit(mapping)
        ? used(mapping.debitColumn, mapping.creditColumn)
        : used(mapping.amountColumn),
    },
    { field: 'description', columns: [...mapping.descriptionColumns] },
    {
      field: 'counterparty',
      columns: used(mapping.counterpartyColumn, mapping.counterpartyIbanColumn),
    },
    { field: 'currency', columns: used(mapping.currencyColumn) },
    { field: 'balance', columns: used(mapping.balanceColumn) },
    { field: 'transactionId', columns: used(mapping.transactionIdColumn) },
  ];
}

/** Whether the inference found something for a field: "not found" against "left out by the person". */
export function wasInferred(field: RecognitionField, inferred: InferredColumns): boolean {
  switch (field) {
    case 'date':
      return inferred.date !== null;
    case 'amount':
      return inferred.amount !== null || (inferred.debit !== null && inferred.credit !== null);
    case 'description':
      return inferred.description.length > 0;
    case 'counterparty':
      return inferred.counterparty !== null || inferred.counterpartyIban !== null;
    case 'currency':
      return inferred.currency !== null;
    case 'balance':
      return inferred.balance !== null;
    case 'transactionId':
      return inferred.transactionId !== null;
  }
}

/**
 * What the running balance said about the signs, as long as the amount is still the one it
 * checked: another column chosen by the person was not checked by anything.
 */
export function signCheckFor(mapping: MappingState, inferred: InferredColumns): SignCheck {
  const sameSingle =
    mapping.amountColumn !== '' && mapping.amountColumn === inferred.amount?.column;
  const samePair =
    mapping.amountColumn === '' &&
    mapping.debitColumn !== '' &&
    mapping.debitColumn === inferred.debit?.column &&
    mapping.creditColumn === inferred.credit?.column;
  return sameSingle || samePair ? inferred.signCheck : 'unchecked';
}

/**
 * Moony cannot import the file as it is read now: the running balance proved the amount
 * column's signs reversed (expenses positive, as card statements print them), and the import
 * cannot flip them. Another amount column, or a pair, lifts it.
 */
export function signsBlockImport(mapping: MappingState, inferred: InferredColumns): boolean {
  return signCheckFor(mapping, inferred) === 'inverted';
}

// ── Where the recognition starts ────────────────────────────────────────────

export type RecognitionStep =
  { stage: 'recognize' } | { stage: 'correct'; field: RecognitionField };

/** The date is the inferred column and its day and month cannot be told from the values. */
function dateOrderOpen(mapping: MappingState, inferred: InferredColumns): boolean {
  return (
    inferred.dateAmbiguous && inferred.date !== null && mapping.dateColumn === inferred.date.column
  );
}

/**
 * Where the recognition opens: on the date's correction when there is no date, on the amount's
 * when there is neither an amount nor a pair (the app never guesses either silently), on the
 * date's again until the day and month order of an ambiguous date was confirmed; else on the
 * three rows and the list of what was recognized.
 */
export function recognitionStart(
  mapping: MappingState,
  inferred: InferredColumns,
  orderConfirmed = false
): RecognitionStep {
  if (mapping.dateColumn === '') return { stage: 'correct', field: 'date' };
  if (!canImport(mapping)) return { stage: 'correct', field: 'amount' };
  if (!orderConfirmed && dateOrderOpen(mapping, inferred)) {
    return { stage: 'correct', field: 'date' };
  }
  return { stage: 'recognize' };
}

/** Whether the mapping has what a required field needs (the others are optional and always "filled"). */
function isFilled(field: RecognitionField, mapping: MappingState): boolean {
  if (field === 'date') return mapping.dateColumn !== '';
  if (field === 'amount') {
    return (
      mapping.amountColumn !== '' || (mapping.debitColumn !== '' && mapping.creditColumn !== '')
    );
  }
  return true;
}

/**
 * What follows a correction the recognition itself opened (`asked`): the next question while the
 * date or the amount is still missing (the date, then the amount), then the order of an ambiguous
 * date, else the rows. A file is no statement only when the field that was just asked is still
 * empty — which "Použít" cannot leave: in practice the person cancelling.
 */
export function afterStartApply(
  mapping: MappingState,
  inferred: InferredColumns,
  orderConfirmed: boolean,
  asked: RecognitionField
): RecognitionStep | { stage: 'unreadable' } {
  if (!isFilled(asked, mapping)) return { stage: 'unreadable' };
  return recognitionStart(mapping, inferred, orderConfirmed);
}

// ── The day and month order ─────────────────────────────────────────────────

export type DayMonthOrder = 'dmy' | 'mdy';

/** A chrono format with day and month exchanged (`%d.%m.%Y` ↔ `%m.%d.%Y`). */
export function swapDayMonth(format: string): string {
  return format.replace(/%([dm])/g, (_, letter: string) => (letter === 'd' ? '%m' : '%d'));
}

/** Which of day and month a numeric format writes first; `null` when it has not both. */
export function dayMonthOrder(format: string): DayMonthOrder | null {
  const day = format.indexOf('%d');
  const month = format.indexOf('%m');
  if (day < 0 || month < 0) return null;
  return day < month ? 'dmy' : 'mdy';
}

/**
 * The order the date column is read in, when it was a question — the inferred column's, or a
 * column the person chose whose dates read both ways (it then carries an explicit format, any
 * other column's is left to the import to detect); `null` when it never was.
 */
export function dateOrderOf(
  mapping: MappingState,
  inferred: InferredColumns
): DayMonthOrder | null {
  const asked =
    dateOrderOpen(mapping, inferred) ||
    (mapping.dateFormat !== AUTO_VALUE && mapping.dateColumn !== inferred.date?.column);
  return asked ? dayMonthOrder(mapping.dateFormat) : null;
}

/** `dd.mm.yyyy`, `d/m/yy`, `dd. mm. yyyy`: three numbers, one separator, a year last (a time may follow). */
const NUMERIC_DATE = /^\s*(\d{1,2})([./-]\s?)(\d{1,2})\2(\d{4}|\d{2})(?:[\sT].*)?$/;

/**
 * The day-first format of a column whose dates read both ways: every cell is such a date with
 * the day and the month both at most 12. `null` for anything else — a column that is no date, one
 * a value above 12 decides, or one whose cells do not agree. The import detects those itself.
 * Only the sample rows are seen: a file whose later rows decide is asked once too often, never
 * guessed.
 */
export function ambiguousDateFormat(cells: readonly string[]): string | null {
  const filled = cells.map((cell) => cell.trim()).filter((cell) => cell !== '');
  let separator = '';
  let year = '';
  for (const cell of filled) {
    const match = NUMERIC_DATE.exec(cell);
    if (!match) return null;
    const [, first, sep, second, y] = match;
    if (separator !== '' && (sep !== separator || y.length !== year.length)) return null;
    separator = sep;
    year = y;
    if ([first, second].some((part) => Number(part) < 1 || Number(part) > 12)) return null;
  }
  return filled.length === 0
    ? null
    : `%d${separator}%m${separator}${year.length === 4 ? '%Y' : '%y'}`;
}

/** The pieces of a chrono format `parseSampleDate` understands. */
const FORMAT_TOKENS: Record<string, string> = {
  '%d': '(\\d{1,2})',
  '%m': '(\\d{1,2})',
  '%Y': '(\\d{4})',
  '%y': '(\\d{2})',
};

const escapeRegex = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/**
 * A sample cell read in a numeric chrono format (`%d %m %Y %y` and the separators between them)
 * as the unix seconds of its UTC day; a time after the date is ignored. `null` for anything else —
 * a format with month names, a cell that is no date, a day the month does not have.
 */
export function parseSampleDate(cell: string, format: string): number | null {
  const parts = format.split(/(%[A-Za-z])/).filter((part) => part !== '');
  const order: string[] = [];
  let pattern = '^\\s*';
  for (const part of parts) {
    if (part.startsWith('%')) {
      const token = FORMAT_TOKENS[part];
      if (!token) return null;
      order.push(part);
      pattern += token;
    } else {
      pattern += escapeRegex(part);
    }
  }
  const match = new RegExp(`${pattern}(?:[\\sT].*)?$`).exec(cell);
  if (!match) return null;

  let year = NaN;
  let month = NaN;
  let day = NaN;
  order.forEach((token, index) => {
    const value = Number(match[index + 1]);
    if (token === '%d') day = value;
    else if (token === '%m') month = value;
    else if (token === '%Y') year = value;
    else year = value < 70 ? 2000 + value : 1900 + value; // chrono's %y
  });
  if (![year, month, day].every(Number.isFinite)) return null;
  const date = new Date(Date.UTC(year, month - 1, day));
  const real =
    date.getUTCFullYear() === year && date.getUTCMonth() === month - 1 && date.getUTCDate() === day;
  return real ? date.getTime() / 1000 : null;
}

// ── The per-field correction ────────────────────────────────────────────────

/** One answer to "which column is the …?": what it shows and what it does to the mapping. */
export interface ColumnOption {
  id: string;
  /**
   * `column` one column, `columns` several that are read together (a description), `pair` a debit
   * and a credit column, `order` the date column read day-first or month-first, `none` "nepoužít".
   */
  kind: 'column' | 'columns' | 'pair' | 'order' | 'none';
  /** The header(s) it names, joined with " + "; `''` for `none`. */
  label: string;
  /** Its sample values as the file has them (a pair: "debit / credit"). */
  samples: string[];
  /** `order` options: the date of a sample cell as this order reads it, unix seconds of the UTC day. */
  dates?: number[];
  order?: DayMonthOrder;
  /** `pair` options: which column is read as the money out (debit) and which as the money in. */
  sides?: { debit: string; credit: string };
  /** Its cells are amounts: they are masked while amounts are hidden. */
  money: boolean;
  apply: Partial<MappingState>;
}

const MONEY_FIELDS: ReadonlySet<RecognitionField> = new Set(['amount', 'balance']);

function cellOf(inspection: CsvPreviewResult, row: readonly string[], header: string): string {
  const index = inspection.headers.indexOf(header);
  return index < 0 ? '' : (row[index] ?? '').trim();
}

/** The first non-empty values of a column, as the file has them. */
function sampleCells(inspection: CsvPreviewResult, header: string): string[] {
  const cells: string[] = [];
  for (const row of inspection.sampleRows) {
    const cell = cellOf(inspection, row, header);
    if (cell !== '') cells.push(cell);
    if (cells.length === SAMPLES_PER_COLUMN) break;
  }
  return cells;
}

/** The first rows that have something in any of `headers`, each rendered from its cells. */
function sampleLines(
  inspection: CsvPreviewResult,
  headers: readonly string[],
  render: (cells: string[]) => string
): string[] {
  const lines: string[] = [];
  for (const row of inspection.sampleRows) {
    const cells = headers.map((header) => cellOf(inspection, row, header));
    if (cells.every((cell) => cell === '')) continue;
    lines.push(render(cells));
    if (lines.length === SAMPLES_PER_COLUMN) break;
  }
  return lines;
}

const MINUSES = /[\u2212\u2012\u2013]/g;
/** `10.10.2026`, `2026-10-10`, `3/4/26`, `10.10.`: three numbers between one kind of separator. */
const DATE_LIKE = /^\d{1,4}([./-])\d{1,2}\1\d{0,4}$/;

/**
 * A cell read as a number the way banks write one: a sign or brackets, digit groups, a decimal
 * mark, a currency before or after it, `1500,-`. `marked` is whether the cell says it is an
 * amount beyond its digits (a sign, a mark, a thousands space, a currency); a plain integer
 * ("9001", "26100000001") may be an amount, an id or a symbol and says nothing. A date is no
 * number. `null` for anything that is not one.
 */
function readNumber(cell: string): { body: string; marked: boolean } | null {
  let text = cell.trim().replace(MINUSES, '-');
  let marked = false;
  const strip = (pattern: RegExp) => {
    const next = text.replace(pattern, '');
    if (next !== text) marked = true;
    text = next;
  };
  strip(/^(?:\p{Sc}|[A-Za-z]{3}(?=\s))\s*/u); // "€ 12,50", "CZK 5 000"
  strip(/\s*(?:\p{Sc}|\p{L}{1,4}\.?)$/u); // "1 500 Kč", "12,50 €"
  strip(/[,.]-$/); // "1500,-"
  if (/\d[\s'’]\d{3}(?!\d)/.test(text)) marked = true; // "5 000", "1'240"
  const body = text.replace(/[\s'’]/g, '');
  if (!/^[+-]?\(?\d[\d.,]*\)?$/.test(body) || DATE_LIKE.test(body)) return null;
  return { body, marked: marked || /^[+(-]|[.,]/.test(body) };
}

const isNumericCell = (cell: string) => readNumber(cell) !== null;
const isNonZero = (cell: string) => /[1-9]/.test(readNumber(cell)?.body ?? '');

/**
 * Whether a cell says it is an amount: a number with a sign, a decimal mark, a thousands space, a
 * currency or `,-` ("−1 249,00", "5 000", "1500,-", "-1249"). A date, an account number and a
 * plain integer — which may as well be an id — do not.
 */
export function readsAsAmount(cell: string): boolean {
  return readNumber(cell)?.marked ?? false;
}

/**
 * A sample cell as the correction shows it while amounts are hidden: every digit of a cell of a
 * money column becomes the mask, and in any other column a cell that says it is an amount does
 * too. Text, dates and plain integers stay, so the columns can still be told apart.
 */
export function maskedSample(cell: string, options: { money: boolean }): string {
  return options.money || readsAsAmount(cell) ? maskMoney(cell) : cell;
}

/** A column all of whose sample cells are numbers: what an amount or a balance could be. */
function isNumericColumn(inspection: CsvPreviewResult, header: string): boolean {
  const cells = inspection.sampleRows
    .map((row) => cellOf(inspection, row, header))
    .filter((cell) => cell !== '');
  return cells.length > 0 && cells.every(isNumericCell);
}

/**
 * Whether the import reads a cell as negative (`clean_and_parse_amount`): a minus before or after
 * the number, or brackets around it, whatever currency or letters stand outside them.
 */
function readsNegative(cell: string): boolean {
  const core = cell.trim().replace(/^[\s\p{L}\p{Sc}]+|[\s\p{L}\p{Sc}]+$/gu, '');
  return /^[(\-−‒–]|[)\-−‒–]$/u.test(core);
}

/**
 * The single amount column the person chose holds only positive amounts in the sample rows and
 * nothing proves its signs: every expense would be saved as income. Not for a pair (it reads its
 * sides), nor for the inferred column — the inference read it over many more rows, and took it
 * only with both signs or with a balance that proved them (any other column is `unchecked`).
 */
export function amountsAllPositive(
  mapping: MappingState,
  inferred: InferredColumns,
  inspection: CsvPreviewResult
): boolean {
  const column = mapping.amountColumn;
  if (column === '' || column === inferred.amount?.column) return false;
  const cells = inspection.sampleRows
    .map((row) => cellOf(inspection, row, column))
    .filter((cell) => cell !== '');
  return (
    cells.length > 0 &&
    cells.every(isNumericCell) &&
    cells.some(isNonZero) &&
    !cells.some(readsNegative)
  );
}

/**
 * The columns whose cells are masked while amounts are hidden: what the inference took as an
 * amount, a debit, a credit or a balance, what the mapping reads as one now (the person may have
 * corrected it), the columns of a pair that looks like a debit and a credit, and every numeric
 * column — any of them may be the answer — on the question of an amount or a balance and while
 * there is no amount yet (the date question of a file missing both: an unsigned whole-number
 * amount such as "25000" says nothing of itself).
 */
function moneyColumnsOf(
  field: RecognitionField,
  inspection: CsvPreviewResult,
  headers: readonly string[],
  inferred: InferredColumns,
  mapping: MappingState | undefined,
  pairs: ReadonlyArray<{ debit: string; credit: string }>
): Set<string> {
  const amountless = mapping ? !isFilled('amount', mapping) : !wasInferred('amount', inferred);
  const money = new Set<string>();
  for (const found of [inferred.amount, inferred.debit, inferred.credit, inferred.balance]) {
    if (found !== null) money.add(found.column);
  }
  if (mapping) {
    for (const name of [
      mapping.amountColumn,
      mapping.debitColumn,
      mapping.creditColumn,
      mapping.balanceColumn,
    ]) {
      if (name !== '') money.add(name);
    }
  }
  for (const pair of pairs) {
    money.add(pair.debit);
    money.add(pair.credit);
  }
  if (MONEY_FIELDS.has(field) || amountless) {
    for (const header of headers) if (isNumericColumn(inspection, header)) money.add(header);
  }
  return money;
}

/**
 * Two neighbouring numeric columns of which exactly one is filled on every row, as a debit and a
 * credit: the way round a header names (one side named is enough, as in the inference), else
 * both ways round — nothing tells which of them is the money out, so the person says it.
 */
function pairCandidates(
  inspection: CsvPreviewResult,
  headers: readonly string[]
): Array<{ debit: string; credit: string }> {
  const numeric = headers.filter((header) => {
    const cells = inspection.sampleRows
      .map((row) => cellOf(inspection, row, header))
      .filter((cell) => cell !== '');
    return cells.length > 0 && cells.every(isNumericCell);
  });
  const named = inspection.suggestedMappings ?? {};
  const [debitHint, creditHint] = [named.debit?.[0], named.credit?.[0]];
  const found: Array<Array<{ debit: string; credit: string }>> = [];
  for (let i = 0; i + 1 < numeric.length; i++) {
    const [left, right] = [numeric[i], numeric[i + 1]];
    let leftRows = 0;
    let rightRows = 0;
    let overlap = false;
    for (const row of inspection.sampleRows) {
      const a = isNonZero(cellOf(inspection, row, left));
      const b = isNonZero(cellOf(inspection, row, right));
      if (a && b) overlap = true;
      if (a) leftRows++;
      if (b) rightRows++;
    }
    if (overlap || leftRows === 0 || rightRows === 0) continue;
    const leftOut = { debit: left, credit: right };
    const rightOut = { debit: right, credit: left };
    const leftNamed = debitHint === left || creditHint === right;
    const rightNamed = debitHint === right || creditHint === left;
    // Without a hint the money out is offered on the left first, as most banks print a pair
    if (leftNamed !== rightNamed) found.push([rightNamed ? rightOut : leftOut]);
    else found.push([leftOut, rightOut]);
  }
  return found.slice(0, 2).flat();
}

function columnOption(
  field: RecognitionField,
  inspection: CsvPreviewResult,
  header: string,
  money: ReadonlySet<string>,
  inferred: InferredColumns
): ColumnOption {
  const base = {
    id: `col:${header}`,
    kind: 'column' as const,
    label: header,
    samples: sampleCells(inspection, header),
    money: money.has(header),
  };
  switch (field) {
    case 'date':
      return {
        ...base,
        apply: {
          dateColumn: header,
          // Only the inferred column's format is known; the import detects any other's
          dateFormat:
            header === inferred.date?.column ? (inferred.dateFormat ?? AUTO_VALUE) : AUTO_VALUE,
        },
      };
    case 'amount':
      return { ...base, apply: { amountColumn: header, debitColumn: '', creditColumn: '' } };
    case 'description':
      return { ...base, apply: { descriptionColumns: [header] } };
    case 'counterparty':
      return { ...base, apply: { counterpartyColumn: header } };
    case 'currency':
      return { ...base, apply: { currencyColumn: header } };
    case 'balance':
      return { ...base, apply: { balanceColumn: header } };
    case 'transactionId':
      return { ...base, apply: { transactionIdColumn: header } };
  }
}

const CLEAR: Record<Exclude<RecognitionField, 'date' | 'amount'>, Partial<MappingState>> = {
  description: { descriptionColumns: [] },
  counterparty: { counterpartyColumn: '', counterpartyIbanColumn: '' },
  currency: { currencyColumn: '' },
  balance: { balanceColumn: '' },
  transactionId: { transactionIdColumn: '' },
};

/**
 * A date column read day-first (`guess`) and month-first, with one sample date for each. The
 * options carry the column's name in their ids: more than one column may read both ways.
 */
function orderOptions(inspection: CsvPreviewResult, header: string, guess: string): ColumnOption[] {
  const other = swapDayMonth(guess);
  const guessOrder = dayMonthOrder(guess);
  if (guessOrder === null || other === guess) return [];

  // A date whose day and month differ tells the two readings apart; one like 04.04. cannot
  const cells = inspection.sampleRows
    .map((row) => cellOf(inspection, row, header))
    .filter((cell) => cell !== '');
  const told = cells.find((cell) => {
    const day = parseSampleDate(cell, guess);
    return day !== null && day !== parseSampleDate(cell, other);
  });
  const cell = told ?? cells.find((candidate) => parseSampleDate(candidate, guess) !== null);
  const reading = (format: string): number[] => {
    const day = cell === undefined ? null : parseSampleDate(cell, format);
    return day === null ? [] : [day];
  };
  const option = (order: DayMonthOrder, format: string): ColumnOption => ({
    id: `order:${header}:${order}`,
    kind: 'order',
    label: header,
    samples: [],
    dates: reading(format),
    order,
    money: false,
    apply: { dateColumn: header, dateFormat: format },
  });
  return [option(guessOrder, guess), option(guessOrder === 'dmy' ? 'mdy' : 'dmy', other)];
}

/**
 * The answers to "which column is the …?": every column of the file with its header and two
 * sample values (file order), and next to them what the field also allows — for the amount the
 * debit and credit pair, its sides named (the inferred one the way the inference oriented it, else
 * the neighbours that look like one: the way a header names, or both ways round), for a date
 * the day-first and month-first readings of every column whose dates read both ways (the
 * inferred one by the inference's own answer, any other by its sample cells: the app never
 * settles that order silently), for a description the columns it was inferred from together, and
 * for an optional field "nepoužít" first. `mapping` is what the import reads now: the columns it
 * takes as amounts are masked while amounts are hidden.
 */
export function columnOptions(
  field: RecognitionField,
  inspection: CsvPreviewResult,
  inferred: InferredColumns,
  mapping?: MappingState
): ColumnOption[] {
  const headers = uniqueHeaders(inspection.headers);
  const candidates = pairCandidates(inspection, headers);
  const money = moneyColumnsOf(field, inspection, headers, inferred, mapping, candidates);
  const options: ColumnOption[] = [];

  if (!isRequiredField(field)) {
    options.push({
      id: 'none',
      kind: 'none',
      label: '',
      samples: [],
      money: false,
      apply: CLEAR[field as keyof typeof CLEAR],
    });
  }

  if (field === 'amount') {
    const pairs: Array<{ debit: string; credit: string }> = [];
    if (inferred.debit !== null && inferred.credit !== null) {
      pairs.push({ debit: inferred.debit.column, credit: inferred.credit.column });
    }
    for (const candidate of candidates) {
      if (
        !pairs.some((pair) => pair.debit === candidate.debit && pair.credit === candidate.credit)
      ) {
        pairs.push(candidate);
      }
    }
    for (const { debit, credit } of pairs) {
      options.push({
        id: `pair:${debit}|${credit}`,
        kind: 'pair',
        label: `${debit} + ${credit}`,
        sides: { debit, credit },
        samples: sampleLines(
          inspection,
          [debit, credit],
          ([out, into]) => `${out || '—'} / ${into || '—'}`
        ),
        money: true,
        apply: { amountColumn: '', debitColumn: debit, creditColumn: credit },
      });
    }
  }

  if (field === 'description') {
    const together = inferred.description.filter((name) => headers.includes(name));
    if (together.length > 1) {
      options.push({
        id: `cols:${together.join('|')}`,
        kind: 'columns',
        label: together.join(' + '),
        samples: sampleLines(inspection, together, (cells) =>
          cells.filter((cell) => cell !== '').join(' · ')
        ),
        money: false,
        apply: { descriptionColumns: together },
      });
    }
  }

  for (const header of headers) {
    // The inference knows its own column's order from up to 200 rows; any other from the sample rows
    const guess =
      field !== 'date'
        ? null
        : header === inferred.date?.column
          ? inferred.dateAmbiguous
            ? inferred.dateFormat
            : null
          : ambiguousDateFormat(
              inspection.sampleRows.map((row) => cellOf(inspection, row, header))
            );
    const ordered = guess ? orderOptions(inspection, header, guess) : [];
    if (ordered.length > 0) options.push(...ordered);
    else options.push(columnOption(field, inspection, header, money, inferred));
  }
  return options;
}

const sameValue = (a: unknown, b: unknown): boolean =>
  Array.isArray(a) && Array.isArray(b)
    ? a.length === b.length && a.every((value, index) => value === b[index])
    : a === b;

/** The id of the option whose answer the mapping already has; `null` when it has none of them. */
export function selectedOption(
  options: readonly ColumnOption[],
  mapping: MappingState
): string | null {
  const found = options.find((option) =>
    (Object.keys(option.apply) as Array<keyof MappingState>).every((key) =>
      sameValue(option.apply[key], mapping[key])
    )
  );
  return found?.id ?? null;
}

/** The mapping with an option's answer written over it. */
export function applyOption(
  mapping: MappingState,
  option: Pick<ColumnOption, 'apply'>
): MappingState {
  return { ...mapping, ...option.apply };
}
