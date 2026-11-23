import { describe, expect, it } from 'vitest';
import type {
  StockReportBucket,
  StockReportLedgerRow,
  StockReportMatch,
  StockReportOpenLot,
  StockReportSale,
} from '@shared/schema';
import {
  holdingSpan,
  ledgerRows,
  openLotGroups,
  openLotsByTicker,
  parseHoldingMonths,
  saleViews,
  totalOf,
  yearPeriod,
} from './stock-transactions-report';

const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;

function match(over: Partial<StockReportMatch> = {}): StockReportMatch {
  return {
    buyDay: day(2020, 1, 1),
    quantity: '10',
    buyPrice: '100',
    buyCurrency: 'USD',
    buyRate: '20',
    averageCost: null,
    proceeds: '3000',
    cost: '2000',
    fees: '50',
    resultAfterFees: '950',
    exempt: false,
    ...over,
  };
}

function sale(over: Partial<StockReportSale> = {}): StockReportSale {
  return {
    id: 's1',
    investmentId: 'i1',
    ticker: 'AAPL',
    companyName: 'Apple Inc.',
    day: day(2026, 3, 10),
    quantity: '10',
    price: '150',
    currency: 'USD',
    rate: '20',
    uncoveredQuantity: '0',
    counted: true,
    matches: [match()],
    ...over,
  };
}

const mixed = sale({
  id: 'mixed',
  ticker: 'MSFT',
  companyName: 'Microsoft',
  quantity: '25',
  matches: [
    match({
      quantity: '20',
      proceeds: '8000',
      cost: '4000',
      fees: '100',
      resultAfterFees: '3900',
      exempt: true,
    }),
    match({
      quantity: '5',
      proceeds: '2000',
      cost: '1500',
      fees: '20',
      resultAfterFees: '480',
      exempt: false,
    }),
  ],
});

const noFilter = { search: '', bucket: null, saleClass: 'all' as const };

describe('saleViews', () => {
  it('searches ticker and company name, case-insensitive and trimmed', () => {
    const sales = [sale(), mixed];
    expect(saleViews(sales, { ...noFilter, search: ' aapl ' }).map((v) => v.sale.id)).toEqual([
      's1',
    ]);
    expect(saleViews(sales, { ...noFilter, search: 'MICRO' }).map((v) => v.sale.id)).toEqual([
      'mixed',
    ]);
  });

  it('keeps sales inside the bucket, both ends included', () => {
    const bucket: StockReportBucket = {
      key: '2026-03',
      fromDay: day(2026, 3, 1),
      toDay: day(2026, 3, 10),
      buys: '0',
      sells: '0',
      result: '0',
    };
    const early = sale({ id: 'early', day: day(2026, 2, 28) });
    expect(saleViews([sale(), early], { ...noFilter, bucket }).map((v) => v.sale.id)).toEqual([
      's1',
    ]);
  });

  it('keeps only the taxable match of a mixed sale and its sums', () => {
    const [view] = saleViews([mixed], { ...noFilter, saleClass: 'taxable' });
    expect(view.matches).toHaveLength(1);
    expect(view.quantity).toBe(5);
    expect(view.proceeds).toBe(2000);
    expect(view.resultAfterFees).toBe(480);
  });

  it('drops a fully taxable sale for the exempt class', () => {
    expect(saleViews([sale()], { ...noFilter, saleClass: 'exempt' })).toEqual([]);
  });

  it('marks a mixed sale with its exempt quantity', () => {
    const [view] = saleViews([mixed], noFilter);
    expect(view.kind).toBe('mixed');
    expect(view.exemptQuantity).toBe(20);
    expect(view.quantity).toBe(25);
  });

  it('has no kind when the test is off', () => {
    const off = sale({ matches: [match({ exempt: null })] });
    expect(saleViews([off], noFilter)[0].kind).toBeNull();
  });

  it('lists an uncounted sale with zero sums', () => {
    const [view] = saleViews(
      [sale({ counted: false, matches: [match({ proceeds: null })] })],
      noFilter
    );
    expect(view.proceeds).toBe(0);
    expect(view.quantity).toBe(0);
    expect(view.kind).toBe('taxable');
  });

  it('adds null amounts as nothing', () => {
    const [view] = saleViews([sale({ matches: [match({ fees: null })] })], noFilter);
    expect(view.fees).toBe(0);
  });
});

describe('totalOf', () => {
  it('adds the views', () => {
    const views = saleViews([sale(), mixed], noFilter);
    expect(totalOf(views)).toEqual({
      proceeds: 13000,
      cost: 7500,
      fees: 170,
      resultAfterFees: 5330,
    });
  });
});

function row(over: Partial<StockReportLedgerRow> = {}): StockReportLedgerRow {
  return {
    id: 'r1',
    kind: 'buy',
    day: day(2026, 1, 5),
    investmentId: 'i1',
    ticker: 'AAPL',
    companyName: 'Apple Inc.',
    quantity: '1',
    price: '100',
    currency: 'USD',
    rate: '20',
    amount: '2000',
    fee: null,
    ...over,
  };
}

describe('ledgerRows', () => {
  const rows = [
    row(),
    row({ id: 'r2', kind: 'sell', ticker: 'MSFT', companyName: 'Microsoft' }),
    row({ id: 'r3', kind: 'dividend' }),
  ];
  it('filters by kind', () => {
    const kept = ledgerRows(rows, { search: '', bucket: null, kind: 'dividend' });
    expect(kept.map((r) => r.id)).toEqual(['r3']);
  });
  it('filters by search', () => {
    const kept = ledgerRows(rows, { search: 'microsoft', bucket: null, kind: 'all' });
    expect(kept.map((r) => r.id)).toEqual(['r2']);
  });
});

describe('yearPeriod', () => {
  it('spans the year in UTC', () => {
    expect(yearPeriod(2026)).toEqual({
      fromDay: Date.UTC(2026, 0, 1) / 1000,
      toDay: Date.UTC(2026, 11, 31) / 1000,
    });
  });
});

describe('holdingSpan', () => {
  it('counts whole years and months', () => {
    expect(holdingSpan(day(2021, 3, 15), day(2026, 2, 10))).toEqual({
      years: 4,
      months: 10,
      days: 0,
    });
  });
  it('counts days under one month', () => {
    expect(holdingSpan(day(2026, 1, 1), day(2026, 1, 13))).toEqual({
      years: 0,
      months: 0,
      days: 12,
    });
  });
});

describe('parseHoldingMonths', () => {
  it('accepts whole numbers 1..600', () => {
    expect(parseHoldingMonths('18')).toBe(18);
    expect(parseHoldingMonths(' 600 ')).toBe(600);
  });
  it('rejects everything else', () => {
    for (const text of ['', '0', '601', '1.5', 'abc']) expect(parseHoldingMonths(text)).toBeNull();
  });
});

function lot(over: Partial<StockReportOpenLot> = {}): StockReportOpenLot {
  return {
    investmentId: 'i1',
    ticker: 'AAPL',
    companyName: 'Apple Inc.',
    buyDay: day(2023, 1, 1),
    quantity: '1',
    lotQuantity: '1',
    buyPrice: '100',
    buyCurrency: 'USD',
    passDay: day(2026, 1, 1),
    value: '1000',
    unrealized: '100',
    ...over,
  };
}

describe('openLotGroups', () => {
  it('splits passed and waiting, sorts them and ignores unpriced lots', () => {
    const today = day(2026, 6, 1);
    const lots = [
      lot({ passDay: day(2027, 5, 1), value: '300' }),
      lot({ passDay: day(2026, 8, 1), value: '200' }),
      lot({ passDay: day(2026, 1, 1), value: '1000' }),
      lot({ passDay: day(2026, 9, 1), value: null }),
    ];
    const groups = openLotGroups(lots, today);
    expect(groups.passed).toHaveLength(1);
    expect(groups.waiting.map((l) => l.passDay)).toEqual([
      day(2026, 8, 1),
      day(2026, 9, 1),
      day(2027, 5, 1),
    ]);
    expect(groups.waitingValue).toBe(500);
    expect(groups.passedValue).toBe(1000);
  });
  it('lists passed lots newest pass first', () => {
    const today = day(2026, 6, 1);
    const groups = openLotGroups(
      [lot({ passDay: day(2025, 1, 1) }), lot({ passDay: day(2026, 2, 1) })],
      today
    );
    expect(groups.passed.map((l) => l.passDay)).toEqual([day(2026, 2, 1), day(2025, 1, 1)]);
  });
});

describe('openLotsByTicker', () => {
  it('groups lots by position in the order of their first lot and keeps the lot order', () => {
    const lots = [
      lot({ investmentId: 'a', ticker: 'AAA', passDay: day(2026, 8, 1), quantity: '5' }),
      lot({ investmentId: 'b', ticker: 'BBB', passDay: day(2026, 9, 1), quantity: '2' }),
      lot({ investmentId: 'a', ticker: 'AAA', passDay: day(2027, 5, 1), quantity: '7' }),
    ];
    const groups = openLotsByTicker(lots);
    expect(groups.map((g) => g.ticker)).toEqual(['AAA', 'BBB']);
    expect(groups[0].lots.map((l) => l.passDay)).toEqual([day(2026, 8, 1), day(2027, 5, 1)]);
    expect(groups[0].quantity).toBe(12);
    expect(groups[1].quantity).toBe(2);
  });
  it('adds up the value and the unrealized result of the priced lots', () => {
    const groups = openLotsByTicker([
      lot({ investmentId: 'a', value: '100.10', unrealized: '10.05' }),
      lot({ investmentId: 'a', value: '200.20', unrealized: '-30.10' }),
    ]);
    expect(groups[0].value).toBeCloseTo(300.3, 6);
    expect(groups[0].unrealized).toBeCloseTo(-20.05, 6);
  });
  it('has no value when none of its lots is priced', () => {
    const groups = openLotsByTicker([
      lot({ investmentId: 'a', value: null, unrealized: null }),
      lot({ investmentId: 'a', value: null, unrealized: null }),
    ]);
    expect(groups[0].value).toBeNull();
    expect(groups[0].unrealized).toBeNull();
  });
});
