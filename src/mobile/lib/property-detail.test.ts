import { describe, expect, it } from 'vitest';
import type {
  AssetValuation,
  InsurancePolicy,
  Loan,
  RealEstate,
  RealEstateOneTimeCost,
  RealEstatePhotoBatch,
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
  costErrors,
  costInput,
  costRows,
  costsMissing,
  galleryPhotos,
  isoToUtcDay,
  latestCostDay,
  linkCandidates,
  monthlyRent,
  oneEstimateOnly,
  photoDeletion,
  policyPremium,
  propertyMissing,
  propertyOwnKeys,
  propertyStatItems,
  propertyTrace,
  recurringInput,
  revalueData,
  todayBatch,
  todayIso,
  traceSpansDays,
  withoutRecurring,
  type CostForm,
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

describe('todayBatch', () => {
  const batch = (id: string, photoDate: number): RealEstatePhotoBatch => ({
    id,
    realEstateId: 'p',
    photoDate,
    description: null,
    photos: [],
    createdAt: photoDate,
  });

  it("is the batch of today's UTC day", () => {
    const batches = [batch('today', TODAY), batch('older', TODAY - 3 * DAY)];
    expect(todayBatch(batches, TODAY)?.id).toBe('today');
  });

  it("is null while no batch has today's day (a photo then starts a new one)", () => {
    expect(
      todayBatch([batch('older', TODAY - DAY), batch('later', TODAY + DAY)], TODAY)
    ).toBeNull();
    expect(todayBatch([], TODAY)).toBeNull();
  });

  it('compares days, not instants: any moment of the day finds the batch of its midnight', () => {
    const batches = [batch('yesterday', TODAY - 1), batch('today', TODAY)];
    expect(todayBatch(batches, NOW)?.id).toBe('today');
    // The last second of yesterday is yesterday's batch, never today's
    expect(todayBatch([batch('yesterday', TODAY - 1)], TODAY)).toBeNull();
    expect(todayBatch([batch('today', TODAY + 23 * 3600)], TODAY)?.id).toBe('today');
  });

  it('takes the first batch of the day when the desktop made two', () => {
    expect(todayBatch([batch('a', TODAY), batch('b', TODAY)], TODAY)?.id).toBe('a');
  });
});

describe('costRows', () => {
  const money = (amount: number, currency: string) => `${amount} ${currency}`;
  const text = { money, day: (utcDay: number) => `d${utcDay}` };
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

  it('has a row for every recurring cost: name, frequency, amount and the amount of a year', () => {
    const rows = costRows(
      property({
        recurringCosts: [
          { name: 'Fond oprav', amount: 1500, frequency: 'monthly', currency: 'CZK' },
          { name: 'Pojistka', amount: 900, frequency: 'Quarterly', currency: 'EUR' },
          { name: 'Daň', amount: 3200, frequency: 'yearly' },
        ],
      }),
      [],
      text
    ).recurring;
    expect(rows).toEqual([
      {
        index: 0,
        name: 'Fond oprav',
        frequency: 'monthly',
        perYear: 12,
        amount: '1500 CZK',
        yearly: '18000 CZK',
      },
      {
        index: 1,
        name: 'Pojistka',
        frequency: 'quarterly',
        perYear: 4,
        amount: '900 EUR',
        yearly: '3600 EUR',
      },
      // A blank currency is the default one, as everywhere else
      {
        index: 2,
        name: 'Daň',
        frequency: 'yearly',
        perYear: 1,
        amount: '3200 CZK',
        yearly: '3200 CZK',
      },
    ]);
  });

  it('reads a frequency the app does not know as a year and an amount that is no number as nothing', () => {
    const [row] = costRows(
      property({
        recurringCosts: [
          { name: 'Různé', amount: Number.NaN, frequency: 'weekly', currency: 'CZK' },
        ],
      }),
      [],
      text
    ).recurring;
    expect(row.perYear).toBe(1);
    expect(row.amount).toBe('0 CZK');
    expect(row.yearly).toBe('0 CZK');
  });

  it('lists the one-time costs newest first, each in its own currency', () => {
    const rows = costRows(
      property(),
      [
        cost({ id: 'old', name: 'Okna', date: day(2022, 3, 1), amount: '80000' }),
        cost({
          id: 'new',
          name: 'Střecha',
          date: day(2025, 8, 9),
          description: 'Pokrývač',
          currency: 'EUR',
          amount: '12000',
        }),
        cost({ id: 'mid', name: 'Kotel', date: day(2023, 11, 20), amount: '45000.50' }),
      ],
      text
    ).oneTime;
    expect(rows.map((row) => row.id)).toEqual(['new', 'mid', 'old']);
    expect(rows[0]).toMatchObject({
      name: 'Střecha',
      description: 'Pokrývač',
      day: `d${day(2025, 8, 9)}`,
      amount: '12000 EUR',
    });
    expect(rows[1].amount).toBe('45000.5 CZK');
  });

  it('puts the later-saved of two costs of one day first, and keeps the cost itself for the edit sheet', () => {
    const first = cost({ id: 'a', createdAt: 1 });
    const second = cost({ id: 'b', createdAt: 2 });
    const rows = costRows(property(), [first, second], text).oneTime;
    expect(rows.map((row) => row.id)).toEqual(['b', 'a']);
    expect(rows[1].cost).toBe(first);
  });

  it('is empty for a property without costs, and does not touch the costs it was given', () => {
    const costs = [
      cost({ id: 'x', date: day(2020, 1, 1) }),
      cost({ id: 'y', date: day(2021, 1, 1) }),
    ];
    expect(costRows(property(), [], text)).toEqual({ recurring: [], oneTime: [] });
    costRows(property(), costs, text);
    expect(costs.map((c) => c.id)).toEqual(['x', 'y']);
  });
});

describe('withoutRecurring', () => {
  const costs = [
    { name: 'Energie', amount: 2000, frequency: 'monthly', currency: 'CZK' },
    { name: 'Fond oprav', amount: 1500, frequency: 'monthly', currency: 'CZK' },
    { name: 'Fond oprav', amount: 1500, frequency: 'monthly', currency: 'CZK' },
    { name: 'Daň', amount: 3200, frequency: 'yearly', currency: 'CZK' },
  ];

  it('removes exactly one entry, the one at the index', () => {
    expect(withoutRecurring(costs, 0).map((c) => c.name)).toEqual([
      'Fond oprav',
      'Fond oprav',
      'Daň',
    ]);
    expect(withoutRecurring(costs, 3).map((c) => c.name)).toEqual([
      'Energie',
      'Fond oprav',
      'Fond oprav',
    ]);
  });

  it('removes only one of two identical entries', () => {
    expect(withoutRecurring(costs, 1)).toHaveLength(3);
    expect(withoutRecurring(costs, 1).filter((c) => c.name === 'Fond oprav')).toHaveLength(1);
  });

  it('leaves the list as it was for an index it does not have, and never changes the list it is given', () => {
    expect(withoutRecurring(costs, 4)).toEqual(costs);
    expect(withoutRecurring(costs, -1)).toEqual(costs);
    expect(withoutRecurring(costs, 4)).not.toBe(costs);
    withoutRecurring(costs, 0);
    expect(costs).toHaveLength(4);
  });
});

describe('cost form', () => {
  const form = (partial: Partial<CostForm> = {}): CostForm => ({
    realEstateId: 'p',
    kind: 'oneTime',
    name: ' Nová střecha ',
    amountText: '85 300,50',
    frequency: 'monthly',
    dateIso: '2026-10-20',
    description: ' pokrývač Novák ',
    keptAmount: null,
    originalDateIso: null,
    ...partial,
  });

  describe('costInput', () => {
    it('makes the one-time cost: the amount as decimal text, the day as a UTC-midnight number, trimmed text', () => {
      const input = costInput(form(), 'CZK');
      expect(input).toEqual({
        realEstateId: 'p',
        name: 'Nová střecha',
        description: 'pokrývač Novák',
        amount: '85300.50',
        currency: 'CZK',
        date: day(2026, 10, 20),
      });
      // A number the backend keeps (`utc_day`), never a `Date`
      expect(typeof input?.date).toBe('number');
    });

    it('leaves the description out when it is blank', () => {
      expect(costInput(form({ description: '   ' }), 'CZK')?.description).toBeUndefined();
    });

    it('takes the currency it is given (a cost keeps its own when it is edited)', () => {
      expect(costInput(form(), 'EUR')?.currency).toBe('EUR');
    });

    it('is null for a form that cannot be saved: no name, no amount, a zero or a negative one, no day', () => {
      expect(costInput(form({ name: '   ' }), 'CZK')).toBeNull();
      expect(costInput(form({ amountText: '' }), 'CZK')).toBeNull();
      expect(costInput(form({ amountText: 'abc' }), 'CZK')).toBeNull();
      expect(costInput(form({ amountText: '0' }), 'CZK')).toBeNull();
      expect(costInput(form({ amountText: '-5' }), 'CZK')).toBeNull();
      expect(costInput(form({ dateIso: '' }), 'CZK')).toBeNull();
    });
  });

  describe('an edited cost whose amount the eye hides', () => {
    // The field starts empty while amounts are hidden; the stored amount is kept beside it
    const hiddenEdit = (partial: Partial<CostForm> = {}) =>
      form({ amountText: '', keptAmount: '85300.50', ...partial });

    it('sends the stored amount when the field was not touched', () => {
      expect(costInput(hiddenEdit({ name: 'Nová kuchyně' }), 'CZK')).toEqual({
        realEstateId: 'p',
        name: 'Nová kuchyně',
        description: 'pokrývač Novák',
        amount: '85300.50',
        currency: 'CZK',
        date: day(2026, 10, 20),
      });
    });

    it('sends what was typed instead, as soon as something is typed', () => {
      expect(costInput(hiddenEdit({ amountText: '90 000' }), 'CZK')?.amount).toBe('90000');
    });

    it('does not let the stored amount hide a typed amount that is wrong', () => {
      expect(costInput(hiddenEdit({ amountText: 'abc' }), 'CZK')).toBeNull();
      expect(costInput(hiddenEdit({ amountText: '0' }), 'CZK')).toBeNull();
    });

    it('needs the stored amount to be a positive one as well (an amount of nothing from the computer)', () => {
      expect(costInput(hiddenEdit({ keptAmount: '0' }), 'CZK')).toBeNull();
    });

    it('is a cost without a stored amount and without a typed one that is null, as before', () => {
      expect(costInput(form({ amountText: '', keptAmount: null }), 'CZK')).toBeNull();
    });
  });

  describe('recurringInput', () => {
    it('makes the recurring cost: a number amount, the frequency and the currency', () => {
      expect(recurringInput(form({ kind: 'recurring', frequency: 'quarterly' }), 'CZK')).toEqual({
        name: 'Nová střecha',
        amount: 85300.5,
        frequency: 'quarterly',
        currency: 'CZK',
      });
    });

    it('is null without a name or without a positive amount, and wants no day', () => {
      expect(recurringInput(form({ kind: 'recurring', name: '' }), 'CZK')).toBeNull();
      expect(recurringInput(form({ kind: 'recurring', amountText: '0' }), 'CZK')).toBeNull();
      expect(recurringInput(form({ kind: 'recurring', dateIso: '' }), 'CZK')).not.toBeNull();
    });
  });

  describe('costErrors', () => {
    const TODAY_ISO = '2026-10-24';

    it('has none for a form that can be saved', () => {
      expect(costErrors(form(), TODAY_ISO)).toEqual({});
      expect(costErrors(form({ kind: 'recurring', dateIso: '' }), TODAY_ISO)).toEqual({});
    });

    it("names a missing name and the amount's problem", () => {
      expect(costErrors(form({ name: ' ' }), TODAY_ISO).name).toBe('required');
      expect(costErrors(form({ amountText: '' }), TODAY_ISO).amount).toBe('required');
      expect(costErrors(form({ amountText: '12 abc' }), TODAY_ISO).amount).toBe('invalid');
      expect(costErrors(form({ amountText: '0' }), TODAY_ISO).amount).toBe('notPositive');
      expect(costErrors(form({ amountText: '-3' }), TODAY_ISO).amount).toBe('notPositive');
    });

    it('wants a day for a one-time cost, and not later than today', () => {
      expect(costErrors(form({ dateIso: '' }), TODAY_ISO).date).toBe('required');
      expect(costErrors(form({ dateIso: '2026-10-25' }), TODAY_ISO).date).toBe('future');
      expect(costErrors(form({ dateIso: TODAY_ISO }), TODAY_ISO).date).toBeUndefined();
    });

    it('does not call the amount missing while the stored one stands in for it (hidden amounts)', () => {
      expect(costErrors(form({ amountText: '', keptAmount: '85300.50' }), TODAY_ISO)).toEqual({});
      expect(costErrors(form({ amountText: '', keptAmount: null }), TODAY_ISO).amount).toBe(
        'required'
      );
      expect(costErrors(form({ amountText: 'x', keptAmount: '85300.50' }), TODAY_ISO).amount).toBe(
        'invalid'
      );
      expect(costErrors(form({ amountText: '', keptAmount: '0' }), TODAY_ISO).amount).toBe(
        'notPositive'
      );
    });

    it('lets an edited cost keep its own day even when that day is ahead, and no later one', () => {
      const edited = (dateIso: string) => form({ dateIso, originalDateIso: '2026-11-02' });
      expect(costErrors(edited('2026-11-02'), TODAY_ISO).date).toBeUndefined();
      // Any day up to the stored one is fine, today and the days between
      expect(costErrors(edited('2026-10-30'), TODAY_ISO).date).toBeUndefined();
      expect(costErrors(edited('2026-10-20'), TODAY_ISO).date).toBeUndefined();
      expect(costErrors(edited('2026-11-03'), TODAY_ISO).date).toBe('future');
    });

    it('caps a new cost, and an edited one dated in the past, at today', () => {
      expect(costErrors(form({ dateIso: '2026-10-25' }), TODAY_ISO).date).toBe('future');
      expect(
        costErrors(form({ dateIso: '2026-10-25', originalDateIso: '2026-09-01' }), TODAY_ISO).date
      ).toBe('future');
    });

    it('does not ask a recurring cost for a day', () => {
      expect(
        costErrors(form({ kind: 'recurring', dateIso: '2099-01-01' }), TODAY_ISO).date
      ).toBeUndefined();
    });
  });
});

describe('galleryPhotos', () => {
  const batch = (
    id: string,
    photoDate: number,
    photoIds: string[],
    description: string | null = null
  ): RealEstatePhotoBatch => ({
    id,
    realEstateId: 'p',
    photoDate,
    description,
    photos: photoIds.map((photoId) => ({ id: photoId, batchId: id, createdAt: photoDate })),
    createdAt: photoDate,
  });

  it('lists the photos of the newest batch first, each with its batch, day and description', () => {
    const photos = galleryPhotos([
      batch('old', day(2025, 6, 1), ['o1']),
      batch('new', day(2026, 10, 24), ['n1', 'n2'], 'Po rekonstrukci'),
    ]);
    expect(photos.map((photo) => photo.id)).toEqual(['n1', 'n2', 'o1']);
    expect(photos[0]).toEqual({
      id: 'n1',
      batchId: 'new',
      day: day(2026, 10, 24),
      description: 'Po rekonstrukci',
      last: false,
    });
  });

  it('marks the only photo of a batch as its last, so that deleting it deletes the batch', () => {
    const photos = galleryPhotos([
      batch('one', day(2026, 1, 1), ['a']),
      batch('two', day(2026, 2, 1), ['b', 'c']),
    ]);
    expect(photos.find((photo) => photo.id === 'a')?.last).toBe(true);
    expect(photos.find((photo) => photo.id === 'b')?.last).toBe(false);
    expect(photos.find((photo) => photo.id === 'c')?.last).toBe(false);
  });

  it('puts the later-created of two batches of one day first, and skips a batch without photos', () => {
    const early = { ...batch('early', day(2026, 5, 5), ['e']), createdAt: 1 };
    const late = { ...batch('late', day(2026, 5, 5), ['l']), createdAt: 2 };
    expect(
      galleryPhotos([early, late, batch('empty', day(2026, 9, 9), [])]).map((p) => p.id)
    ).toEqual(['l', 'e']);
    expect(galleryPhotos([])).toEqual([]);
  });
});

describe('latestCostDay', () => {
  const edited = (originalDateIso: string | null) =>
    ({ originalDateIso }) as Pick<CostForm, 'originalDateIso'>;

  it('is today for a new cost and for one dated today or before', () => {
    expect(latestCostDay(edited(null), '2026-10-24')).toBe('2026-10-24');
    expect(latestCostDay(edited('2026-10-24'), '2026-10-24')).toBe('2026-10-24');
    expect(latestCostDay(edited('2025-01-01'), '2026-10-24')).toBe('2026-10-24');
  });

  it('is the stored day of an edited cost that is dated ahead: the picker offers it, and no later one', () => {
    expect(latestCostDay(edited('2026-11-02'), '2026-10-24')).toBe('2026-11-02');
  });
});

describe('photoDeletion', () => {
  const batch = (id: string, photoIds: string[]): RealEstatePhotoBatch => ({
    id,
    realEstateId: 'p',
    photoDate: TODAY,
    description: null,
    photos: photoIds.map((photoId) => ({ id: photoId, batchId: id, createdAt: 0 })),
    createdAt: 0,
  });

  it("deletes the batch with the photo when it is still the batch's only one", () => {
    expect(photoDeletion([batch('b1', ['a']), batch('b2', ['x', 'y'])], 'a')).toEqual({
      kind: 'batch',
      batchId: 'b1',
    });
  });

  it('deletes the photo alone once its batch has gained photos (an upload landed after it was opened)', () => {
    // The thumbnail was tapped when 'a' was the only photo; by the confirm, two more are in
    expect(photoDeletion([batch('b1', ['a', 'n1', 'n2'])], 'a')).toEqual({
      kind: 'photo',
      photoId: 'a',
    });
  });

  it('deletes the photo alone from a batch of several', () => {
    expect(photoDeletion([batch('b2', ['x', 'y'])], 'y')).toEqual({ kind: 'photo', photoId: 'y' });
  });

  it('has nothing to delete for a photo that is gone already', () => {
    expect(photoDeletion([batch('b1', ['a'])], 'zzz')).toEqual({ kind: 'gone' });
    expect(photoDeletion([], 'a')).toEqual({ kind: 'gone' });
  });
});
