import { describe, expect, it } from 'vitest';
import type {
  AssetValuation,
  InsurancePolicy,
  Loan,
  RealEstate,
  RealEstateOneTimeCost,
} from '@shared/schema';
import {
  propertyRow,
  yearlyBalance,
  type PropertyRow,
  type YearlyBalance,
} from '@/utils/real-estate';
import {
  appreciationOf,
  balanceNote,
  balanceRows,
  costsMissing,
  isoToUtcDay,
  linkCandidates,
  monthlyRent,
  oneEstimateOnly,
  policyPremium,
  propertyMissing,
  propertyOwnKeys,
  propertyStatItems,
  propertyTrace,
  revalueData,
  todayIso,
  traceSpansDays,
} from './property-detail';

const DAY = 86_400;
const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;
const TODAY = day(2026, 10, 24);
/** Ten in the morning, UTC, on `TODAY`. */
const NOW = TODAY + 10 * 3600;

/** Today: 1 EUR = 25 CZK; no rate for KWD. */
const convert = (amount: number, currency: string) =>
  currency === 'KWD' ? null : currency === 'EUR' ? amount * 25 : amount;
/** Before 1. 1. 2025: 1 EUR = 20 CZK, later 24; never a rate for KWD. */
const convertAt = (amount: number, currency: string, t: number) =>
  currency === 'KWD'
    ? null
    : currency === 'EUR'
      ? amount * (t < day(2025, 1, 1) ? 20 : 24)
      : amount;

function property(partial: Partial<RealEstate> = {}): RealEstate {
  return {
    id: 'p',
    name: 'Byt 2+kk',
    address: 'Lipová 12, Praha',
    type: 'investment',
    purchasePrice: '4000000',
    purchasePriceCurrency: 'CZK',
    purchaseDate: day(2020, 5, 1),
    marketPrice: '5000000',
    marketPriceCurrency: 'CZK',
    monthlyRent: '20000',
    monthlyRentCurrency: 'CZK',
    recurringCosts: [],
    photos: [],
    notes: null,
    createdAt: day(2020, 5, 2),
    updatedAt: 0,
    ...partial,
  };
}

function loan(partial: Partial<Loan> = {}): Loan {
  return {
    id: 'l1',
    name: 'Hypotéka',
    principal: '3000000',
    currency: 'CZK',
    interestRate: '3',
    interestRateValidityDate: null,
    monthlyPayment: '15000',
    startDate: day(2020, 5, 1),
    endDate: null,
    balanceAnchorAmount: null,
    balanceAnchorDate: null,
    outstandingBalance: '2000000',
    balanceMode: 'estimated',
    events: [],
    createdAt: 0,
    updatedAt: 0,
    ...partial,
  };
}

function policy(partial: Partial<InsurancePolicy> = {}): InsurancePolicy {
  return {
    id: 'i1',
    type: 'property',
    provider: 'Kooperativa',
    policyName: 'Pojištění domácnosti',
    policyNumber: null,
    startDate: day(2024, 1, 9),
    endDate: null,
    paymentFrequency: 'quarterly',
    oneTimePayment: null,
    oneTimePaymentCurrency: null,
    regularPayment: '2150',
    regularPaymentCurrency: 'CZK',
    limits: [],
    notes: null,
    status: 'active',
    createdAt: 0,
    updatedAt: 0,
    ...partial,
  };
}

function valuation(partial: Partial<AssetValuation> & { valuedAt: number }): AssetValuation {
  return {
    id: `v-${partial.valuedAt}`,
    assetId: 'p',
    value: '5000000',
    currency: 'CZK',
    note: null,
    createdAt: partial.valuedAt,
    ...partial,
  };
}

const PURCHASE = { day: day(2020, 5, 1), value: 4_000_000 };

describe('propertyTrace', () => {
  const early = valuation({ valuedAt: day(2023, 1, 10), value: '4800000', note: 'makléř' });
  const late = valuation({ valuedAt: day(2025, 6, 1), value: '5500000' });

  it('runs purchase, every estimate and today, with the purchase and each estimate as a mark', () => {
    const trace = propertyTrace([late, early], PURCHASE, NOW, 5_600_000, convertAt);
    expect(trace.points).toEqual([
      { t: PURCHASE.day, value: 4_000_000 },
      { t: early.valuedAt, value: 4_800_000 },
      { t: late.valuedAt, value: 5_500_000 },
      { t: NOW, value: 5_600_000 },
    ]);
    expect(trace.events).toEqual([
      { id: 'purchase', t: PURCHASE.day, type: 'buy', kind: 'purchase', value: 4_000_000 },
      {
        id: early.id,
        t: early.valuedAt,
        type: 'mark',
        kind: 'estimate',
        value: 4_800_000,
        note: 'makléř',
        first: true,
      },
      {
        id: late.id,
        t: late.valuedAt,
        type: 'mark',
        kind: 'estimate',
        value: 5_500_000,
        note: null,
        first: false,
      },
    ]);
    expect(trace.missing).toEqual([]);
  });

  it('keeps one point and one mark per day: the newest estimate of the day wins', () => {
    const morning = valuation({
      id: 'morning',
      valuedAt: day(2024, 3, 1),
      value: '4500000',
      createdAt: 10,
    });
    const evening = valuation({
      id: 'evening',
      valuedAt: day(2024, 3, 1),
      value: '4700000',
      createdAt: 20,
    });
    const trace = propertyTrace([evening, morning], PURCHASE, NOW, 4_700_000, convertAt);
    expect(trace.points.filter((p) => p.t === day(2024, 3, 1))).toEqual([
      { t: day(2024, 3, 1), value: 4_700_000 },
    ]);
    expect(trace.events.filter((e) => e.type === 'mark').map((e) => e.id)).toEqual(['evening']);
  });

  it('lets the purchase mark win on the day of an estimate, with no second point there', () => {
    const sameDay = valuation({ valuedAt: PURCHASE.day, value: '4100000' });
    const trace = propertyTrace([sameDay, late], PURCHASE, NOW, 5_500_000, convertAt);
    expect(trace.points.map((p) => p.t)).toEqual([PURCHASE.day, late.valuedAt, NOW]);
    expect(trace.events.map((e) => e.id)).toEqual(['purchase', late.id]);
    // The purchase stands for the start of the log: nothing after it is called the first estimate
    expect(trace.events.find((e) => e.id === late.id)?.first).toBe(false);
  });

  it('puts the purchase mark on the line even when an estimate predates the purchase day', () => {
    const before = valuation({ valuedAt: day(2019, 1, 1), value: '3500000' });
    const trace = propertyTrace([before], PURCHASE, NOW, 5_000_000, convertAt);
    // The purchase is no point of the line (it does not precede the first estimate) but it is marked
    expect(trace.points.map((p) => p.t)).toEqual([before.valuedAt, NOW]);
    expect(trace.events[0]).toMatchObject({ id: 'purchase', type: 'buy' });
  });

  it('has no purchase mark without its day, its price or a rate for it', () => {
    const marks = (purchase: { day: number | null; value: number | null }) =>
      propertyTrace([early], purchase, NOW, 5_000_000, convertAt).events.map((e) => e.type);
    expect(marks({ day: null, value: 4_000_000 })).toEqual(['mark']);
    expect(marks({ day: PURCHASE.day, value: null })).toEqual(['mark']);
    expect(marks({ day: PURCHASE.day, value: 0 })).toEqual(['mark']);
  });

  it('converts each estimate at the rates of its own day', () => {
    const eur1 = valuation({ valuedAt: day(2024, 6, 1), value: '200000', currency: 'EUR' });
    const eur2 = valuation({ valuedAt: day(2025, 6, 1), value: '200000', currency: 'EUR' });
    const trace = propertyTrace([eur1, eur2], PURCHASE, NOW, 5_000_000, convertAt);
    expect(trace.points.map((p) => p.value)).toEqual([4_000_000, 4_000_000, 4_800_000, 5_000_000]);
  });

  it('leaves an estimate without a rate off the line and names its currency', () => {
    const kwd = valuation({ valuedAt: day(2024, 6, 1), value: '1000', currency: 'KWD' });
    const trace = propertyTrace([kwd, late], PURCHASE, NOW, 5_500_000, convertAt);
    expect(trace.missing).toEqual(['KWD']);
    expect(trace.points.map((p) => p.t)).toEqual([PURCHASE.day, late.valuedAt, NOW]);
    expect(trace.events.map((e) => e.id)).toEqual(['purchase', late.id]);
    // The estimate that is left becomes the first one
    expect(trace.events[1].first).toBe(true);
  });

  it('ends on today at the latest estimate when today has no value', () => {
    const trace = propertyTrace([early, late], PURCHASE, NOW, null, convertAt);
    expect(trace.points[trace.points.length - 1]).toEqual({ t: NOW, value: 5_500_000 });
  });

  it('is empty for a property that has no estimate', () => {
    expect(propertyTrace([], PURCHASE, NOW, 5_000_000, convertAt)).toEqual({
      points: [],
      events: [],
      missing: [],
    });
  });
});

describe('propertyStatItems', () => {
  const rowAndBalance = (
    p: RealEstate,
    loans: Loan[] = [],
    insurances: InsurancePolicy[] = []
  ): { row: PropertyRow; balance: YearlyBalance } => ({
    row: propertyRow(p, loans, false, convert, convertAt),
    balance: yearlyBalance(
      p,
      loans,
      loans.map(() => undefined),
      insurances,
      TODAY,
      convert
    ),
  });
  const stats = (p: RealEstate, loans: Loan[] = [], insurances: InsurancePolicy[] = []) => {
    const { row, balance } = rowAndBalance(p, loans, insurances);
    return propertyStatItems(row, balance);
  };

  it('values the property and sets it against the purchase price', () => {
    const { value } = stats(property());
    expect(value.amount).toBe(5_000_000);
    expect(value.native).toEqual({ amount: 5_000_000, currency: 'CZK' });
    expect(value.appreciation).toBeCloseTo(0.25);
  });

  it('gives a loss against the purchase as a negative ratio', () => {
    expect(stats(property({ marketPrice: '3000000' })).value.appreciation).toBeCloseTo(-0.25);
  });

  it('has no appreciation when the purchase price has no rate, or is nothing', () => {
    expect(
      stats(property({ purchasePrice: '1000', purchasePriceCurrency: 'KWD' })).value.appreciation
    ).toBeNull();
    expect(stats(property({ purchasePrice: '0' })).value.appreciation).toBeNull();
  });

  it('keeps the native value when the market currency has no rate', () => {
    const { value } = stats(property({ marketPrice: '9000', marketPriceCurrency: 'KWD' }));
    expect(value.amount).toBeNull();
    expect(value.native).toEqual({ amount: 9000, currency: 'KWD' });
    expect(value.appreciation).toBeNull();
  });

  it('shows the equity with the loan to value of the linked loans', () => {
    const { equity } = stats(property(), [loan()]);
    expect(equity).toEqual({ amount: 3_000_000, ltv: expect.closeTo(0.4), hasLoan: true });
  });

  it('says there is no loan when the property has none, or only a paid-off one', () => {
    expect(stats(property()).equity).toEqual({ amount: 5_000_000, ltv: 0, hasLoan: false });
    expect(stats(property(), [loan({ outstandingBalance: '0' })]).equity.hasLoan).toBe(false);
  });

  it('leaves the equity unknown when a loan has no rate, never counting it as nothing', () => {
    const { equity } = stats(property(), [
      loan(),
      loan({ id: 'l2', currency: 'KWD', outstandingBalance: '500' }),
    ]);
    expect(equity).toEqual({ amount: null, ltv: null, hasLoan: true });
  });

  it('gives the gross yield with the yearly rent', () => {
    const { yield: gross } = stats(property());
    expect(gross.hasRent).toBe(true);
    expect(gross.ratio).toBeCloseTo(0.048);
    expect(gross.rent).toEqual({ main: 240_000, native: { amount: 240_000, currency: 'CZK' } });
  });

  it('has no yield and no rent for a property that is not rented', () => {
    const { yield: gross } = stats(property({ monthlyRent: '0' }));
    expect(gross).toEqual({ hasRent: false, ratio: null, rent: null });
  });

  it('keeps the yearly rent native when its currency has no rate', () => {
    const { yield: gross } = stats(property({ monthlyRent: '1000', monthlyRentCurrency: 'KWD' }));
    expect(gross.ratio).toBeNull();
    expect(gross.rent).toEqual({ main: null, native: { amount: 12_000, currency: 'KWD' } });
  });

  it('gives the net cashflow of a year, gain when it is positive', () => {
    // Rent 240 000 less the interest 60 000 (3 % of 2 000 000) = 180 000, less the principal 120 000
    const { cashflow } = stats(property(), [loan()]);
    expect(cashflow).toEqual({ amount: 60_000, tone: 'gain' });
  });

  it('colours a negative cashflow as a loss and a zero one not at all', () => {
    expect(stats(property({ monthlyRent: '5000' }), [loan()]).cashflow).toEqual({
      amount: -120_000,
      tone: 'loss',
    });
    expect(stats(property({ monthlyRent: '0' })).cashflow).toEqual({ amount: 0, tone: null });
  });

  it('leaves the cashflow unknown when an amount of it has no rate', () => {
    const { cashflow } = stats(property(), [loan({ currency: 'KWD' })]);
    expect(cashflow).toEqual({ amount: null, tone: null });
  });
});

describe('appreciationOf', () => {
  it('is the change against the purchase price and its ratio', () => {
    const row = propertyRow(property(), [], false, convert, convertAt);
    expect(appreciationOf(row)).toEqual({ delta: 1_000_000, ratio: 0.25 });
  });

  it('is null when either side is unknown or the purchase price is nothing', () => {
    const noRate = propertyRow(
      property({ purchasePrice: '10', purchasePriceCurrency: 'KWD' }),
      [],
      false,
      convert,
      convertAt
    );
    const free = propertyRow(property({ purchasePrice: '0' }), [], false, convert, convertAt);
    expect(appreciationOf(noRate)).toBeNull();
    expect(appreciationOf(free)).toBeNull();
  });
});

describe('monthlyRent', () => {
  const rentOf = (p: RealEstate) => monthlyRent(p, yearlyBalance(p, [], [], [], TODAY, convert));

  it('is a twelfth of the yearly rent in the main currency, with its own as the fallback', () => {
    expect(rentOf(property({ monthlyRent: '16500' }))).toEqual({
      main: 16_500,
      native: { amount: 16_500, currency: 'CZK' },
    });
    expect(rentOf(property({ monthlyRent: '800', monthlyRentCurrency: 'EUR' }))).toEqual({
      main: 20_000,
      native: { amount: 800, currency: 'EUR' },
    });
  });

  it('keeps only the native rent while its currency has no rate', () => {
    expect(rentOf(property({ monthlyRent: '300', monthlyRentCurrency: 'KWD' }))).toEqual({
      main: null,
      native: { amount: 300, currency: 'KWD' },
    });
  });

  it('reads a missing rent as nothing in the default currency', () => {
    expect(rentOf(property({ monthlyRent: null, monthlyRentCurrency: null }))).toEqual({
      main: 0,
      native: { amount: 0, currency: 'CZK' },
    });
  });
});

describe('balanceRows', () => {
  const balanceOf = (p: RealEstate, loans: Loan[] = [], insurances: InsurancePolicy[] = []) =>
    yearlyBalance(
      p,
      loans,
      loans.map(() => undefined),
      insurances,
      TODAY,
      convert
    );

  it('lists the lines of the desktop card: income positive, costs and principal negative', () => {
    const rows = balanceRows(
      balanceOf(
        property({
          recurringCosts: [{ name: 'Fond oprav', amount: 2000, frequency: 'monthly' }],
        }),
        [loan()],
        [policy()]
      )
    );
    expect(rows.map((r) => r.key)).toEqual([
      'rent',
      'recurring',
      'insurance',
      'interest',
      'netIncome',
      'principal',
      'cashflow',
    ]);
    const amount = (key: string) => rows.find((r) => r.key === key)?.amount;
    expect(amount('rent')).toBe(240_000);
    expect(amount('recurring')).toBe(-24_000);
    expect(amount('insurance')).toBe(-8_600);
    expect(amount('interest')).toBe(-60_000);
    expect(amount('netIncome')).toBe(240_000 - 24_000 - 8_600 - 60_000);
    expect(amount('principal')).toBe(-120_000);
    expect(amount('cashflow')).toBe(240_000 - 24_000 - 8_600 - 60_000 - 120_000);
  });

  it('colours the lines as the desktop does and totals the two results', () => {
    const rows = balanceRows(balanceOf(property(), [loan()]));
    const tone = (key: string) => rows.find((r) => r.key === key)?.tone;
    expect(tone('rent')).toBe('gain');
    expect(tone('interest')).toBe('loss');
    expect(tone('principal')).toBe('loss');
    expect(tone('recurring')).toBeUndefined();
    expect(tone('netIncome')).toBe('total');
    expect(tone('cashflow')).toBe('total');
  });

  it('shows a line without a rate as unknown, never as nothing', () => {
    const rows = balanceRows(balanceOf(property(), [loan({ currency: 'KWD' })]));
    const amount = (key: string) => rows.find((r) => r.key === key)?.amount;
    expect(amount('rent')).toBe(240_000);
    expect(amount('interest')).toBeNull();
    expect(amount('netIncome')).toBeNull();
    expect(amount('cashflow')).toBeNull();
  });
});

describe('balanceNote', () => {
  const noteOf = (p: RealEstate, loans: Loan[] = [], insurances: InsurancePolicy[] = []) =>
    balanceNote(
      yearlyBalance(
        p,
        loans,
        loans.map(() => undefined),
        insurances,
        TODAY,
        convert
      )
    );

  it('says there is nothing to balance without rent, costs and payments', () => {
    expect(noteOf(property({ monthlyRent: '0' }))).toEqual({ kind: 'empty' });
  });

  it('says what the property earns a month when the cashflow is not negative', () => {
    expect(noteOf(property(), [loan()])).toEqual({
      kind: 'earns',
      monthly: 5_000,
      income: 180_000,
    });
  });

  it('says what it costs a month when the cashflow is negative', () => {
    expect(noteOf(property({ monthlyRent: '5000' }), [loan()])).toEqual({
      kind: 'costs',
      monthly: 10_000,
      income: 0,
    });
  });

  it('names the currencies without a rate when the result is unknown', () => {
    expect(noteOf(property(), [loan({ currency: 'KWD' })])).toEqual({
      kind: 'missing',
      currencies: ['KWD'],
    });
  });
});

describe('linkCandidates', () => {
  const a = { id: 'a', name: 'A' };
  const b = { id: 'b', name: 'B' };
  const c = { id: 'c', name: 'C' };

  it('drops what is already linked and keeps the order of the rest', () => {
    expect(linkCandidates([a, b, c], [b])).toEqual([a, c]);
  });

  it('offers everything when nothing is linked, and nothing when all are', () => {
    expect(linkCandidates([a, b], [])).toEqual([a, b]);
    expect(linkCandidates([a, b], [a, b])).toEqual([]);
    expect(linkCandidates([], [a])).toEqual([]);
  });
});

describe('policyPremium', () => {
  it('is the regular payment with its frequency, in its own currency', () => {
    expect(policyPremium(policy())).toEqual({
      amount: 2150,
      currency: 'CZK',
      frequency: 'quarterly',
    });
  });

  it('is the lump sum of a one-time policy, in the currency of that sum', () => {
    expect(
      policyPremium(
        policy({
          paymentFrequency: 'one_time',
          regularPayment: '0',
          oneTimePayment: '12000',
          oneTimePaymentCurrency: 'EUR',
        })
      )
    ).toEqual({ amount: 12_000, currency: 'EUR', frequency: 'one_time' });
  });

  it('reads a blank currency as the default one and an unknown frequency as none', () => {
    expect(
      policyPremium(policy({ regularPaymentCurrency: '', paymentFrequency: 'weekly' }))
    ).toEqual({
      amount: 2150,
      currency: 'CZK',
      frequency: null,
    });
  });
});

describe('propertyMissing', () => {
  const base = { fxReady: true, datedReady: true, now: [], dated: [] };

  it('is empty while the current quotes load: a rate that is pending is not missing', () => {
    expect(propertyMissing({ ...base, fxReady: false, now: ['KWD'], dated: ['JPY'] })).toEqual([]);
  });

  it('names the currencies of today’s amounts, each once', () => {
    expect(propertyMissing({ ...base, now: ['KWD', 'JPY', 'KWD'] })).toEqual(['KWD', 'JPY']);
  });

  it('adds the currencies of the dated amounts only once their rates have loaded', () => {
    expect(propertyMissing({ ...base, datedReady: false, now: ['KWD'], dated: ['JPY'] })).toEqual([
      'KWD',
    ]);
    expect(propertyMissing({ ...base, now: ['KWD'], dated: ['JPY', 'KWD'] })).toEqual([
      'KWD',
      'JPY',
    ]);
  });
});

describe('costsMissing', () => {
  const cost = (partial: Partial<RealEstateOneTimeCost>): RealEstateOneTimeCost => ({
    id: 'c',
    realEstateId: 'p',
    name: 'Rekonstrukce',
    description: null,
    amount: '100000',
    currency: 'CZK',
    date: day(2024, 1, 1),
    createdAt: 0,
    ...partial,
  });

  it('names the currency of a cost whose day has no rate', () => {
    expect(
      costsMissing([cost({}), cost({ currency: 'KWD' }), cost({ currency: 'KWD' })], convertAt)
    ).toEqual(['KWD']);
  });

  it('needs no rate for an amount of nothing, nor for the main currency', () => {
    expect(costsMissing([cost({ amount: '0', currency: 'KWD' }), cost({})], convertAt)).toEqual([]);
  });

  it('reads a blank currency as the default one', () => {
    expect(costsMissing([cost({ currency: '' })], convertAt)).toEqual([]);
  });
});

describe('revalueData', () => {
  const form = { valueText: '5 400 000', dateIso: '2026-10-20', note: '  odhad makléře ' };
  const args = { assetId: 'p', currency: 'CZK', today: '2026-10-24' };

  it('makes the valuation: the amount as decimal text, the day as UTC seconds, the trimmed note', () => {
    expect(revalueData(form, args)).toEqual({
      ok: true,
      data: {
        assetId: 'p',
        value: '5400000',
        currency: 'CZK',
        valuedAt: day(2026, 10, 20),
        note: 'odhad makléře',
      },
    });
  });

  it('reads the amount the keypad of the system language types', () => {
    const result = revalueData({ ...form, valueText: '5400000,50' }, args);
    expect(result).toMatchObject({ ok: true, data: { value: '5400000.50' } });
  });

  it('leaves a blank note out (null), as the desktop dialog does', () => {
    expect(revalueData({ ...form, note: '   ' }, args)).toMatchObject({
      ok: true,
      data: { note: null },
    });
  });

  it('accepts today and refuses a day in the future', () => {
    expect(revalueData({ ...form, dateIso: '2026-10-24' }, args).ok).toBe(true);
    expect(revalueData({ ...form, dateIso: '2026-10-25' }, args)).toEqual({
      ok: false,
      errors: { date: 'future' },
    });
  });

  it('refuses a missing date, a missing amount, a text that is no number and a negative one', () => {
    expect(revalueData({ ...form, dateIso: '' }, args)).toEqual({
      ok: false,
      errors: { date: 'required' },
    });
    expect(revalueData({ ...form, valueText: '' }, args)).toEqual({
      ok: false,
      errors: { value: 'required' },
    });
    expect(revalueData({ ...form, valueText: 'abc' }, args)).toEqual({
      ok: false,
      errors: { value: 'invalid' },
    });
    expect(revalueData({ ...form, valueText: '-5' }, args)).toEqual({
      ok: false,
      errors: { value: 'invalid' },
    });
  });

  it('reports every problem at once', () => {
    expect(revalueData({ ...form, valueText: 'x', dateIso: '' }, args)).toEqual({
      ok: false,
      errors: { value: 'invalid', date: 'required' },
    });
  });
});

describe('todayIso and isoToUtcDay', () => {
  it('writes the UTC day of an instant as an ISO date, not the local one', () => {
    expect(todayIso(new Date('2026-10-24T23:30:00Z'))).toBe('2026-10-24');
    expect(todayIso(new Date('2026-10-25T00:30:00Z'))).toBe('2026-10-25');
  });

  it('turns an ISO date into the UTC-midnight second of that day', () => {
    expect(isoToUtcDay('2026-10-24')).toBe(TODAY);
    expect(isoToUtcDay('2026-10-25') - isoToUtcDay('2026-10-24')).toBe(DAY);
  });
});

describe('propertyOwnKeys', () => {
  it('lists every query the property detail keeps under the property’s own id', () => {
    expect(propertyOwnKeys('p1')).toEqual([
      ['real-estate', 'p1'],
      ['real-estate-valuations', 'p1'],
      ['real-estate-costs', 'p1'],
      ['real-estate-loans', 'p1'],
      ['real-estate-insurances', 'p1'],
      ['real-estate-photo-batches', 'p1'],
      ['real-estate-documents', 'p1'],
    ]);
  });
});

describe('traceSpansDays', () => {
  it('is false for no points, one point, and points that all fall on one UTC day', () => {
    expect(traceSpansDays([])).toBe(false);
    expect(traceSpansDays([{ t: TODAY, value: 5_000_000 }])).toBe(false);
    // One estimate dated today and the line's end a few hours later: one day, nothing to draw
    expect(
      traceSpansDays([
        { t: TODAY, value: 5_000_000 },
        { t: NOW, value: 5_000_000 },
      ])
    ).toBe(false);
  });

  it('is true once the points reach two different UTC days, however close', () => {
    expect(
      traceSpansDays([
        { t: TODAY - DAY, value: 4_900_000 },
        { t: NOW, value: 5_000_000 },
      ])
    ).toBe(true);
    // 23:59 and 00:01 across midnight are two days
    expect(
      traceSpansDays([
        { t: TODAY - 60, value: 1 },
        { t: TODAY + 60, value: 1 },
      ])
    ).toBe(true);
  });

  it('follows the trace: a property valued today and never bought has no span, with a purchase it has', () => {
    const today = valuation({ valuedAt: TODAY, value: '5000000' });
    expect(
      traceSpansDays(
        propertyTrace([today], { day: null, value: null }, NOW, 5_000_000, convertAt).points
      )
    ).toBe(false);
    expect(traceSpansDays(propertyTrace([today], PURCHASE, NOW, 5_000_000, convertAt).points)).toBe(
      true
    );
  });
});

describe('oneEstimateOnly', () => {
  const at = (valuedAt: number) => valuation({ valuedAt });
  const eventsOf = (
    valuations: AssetValuation[],
    purchase: { day: number | null; value: number | null } = PURCHASE
  ) => propertyTrace(valuations, purchase, NOW, 5_000_000, convertAt).events;

  it('is true for a trace with one estimate and no purchase mark', () => {
    expect(oneEstimateOnly(eventsOf([at(day(2025, 6, 1))], { day: null, value: null }))).toBe(true);
  });

  it('is false with a purchase mark, with several estimates and with none', () => {
    expect(oneEstimateOnly(eventsOf([at(day(2025, 6, 1))]))).toBe(false);
    expect(
      oneEstimateOnly(
        eventsOf([at(day(2024, 6, 1)), at(day(2025, 6, 1))], { day: null, value: null })
      )
    ).toBe(false);
    expect(oneEstimateOnly([])).toBe(false);
  });
});
