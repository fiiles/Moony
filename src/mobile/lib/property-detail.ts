import type { QueryKey } from '@tanstack/react-query';
import type {
  AssetValuation,
  InsertAssetValuation,
  InsurancePolicy,
  RealEstate,
  RealEstateOneTimeCost,
} from '@shared/schema';
import { parsePaymentFrequency, type PaymentFrequency } from '@shared/calculations';
import { recordCurrency } from '@shared/currencies';
import type { ConvertAtFn } from '@shared/fx';
import { utcDayFloor } from '@/utils/chart-axis';
import type { ChartEvent } from '@/utils/chart-scale';
import type { PropertyRow, YearlyBalance, NativeAmount } from '@/utils/real-estate';
import { convertValuations, dailyValuations, valuationTrace } from '@/utils/valuation-trace';
import { parseAmountInput } from './amount-input';
import type { CostPoint } from './position-series';

/**
 * The property detail's numbers as pure functions (spec §8.8 "Nemovitost"): the valuation trace,
 * the four stats, the yearly balance card, the link candidates and the revaluation form. They are
 * the desktop page's rules (`pages/RealEstateDetail.tsx`) without its strings and formatting; the
 * phone screen words and masks them.
 */

/** A mark on the value trace: the purchase (`buy`, ▲) or an estimate (`mark`, ○). */
export interface TraceEvent extends ChartEvent {
  type: 'buy' | 'mark';
  kind: 'purchase' | 'estimate';
  /** In the main currency. */
  value: number;
  note?: string | null;
  /** The earliest estimate: the one that starts the log. */
  first?: boolean;
}

export interface PropertyTrace {
  points: CostPoint[];
  events: TraceEvent[];
  /** Currencies of the estimates whose day has no rate (they are left off the line). */
  missing: string[];
}

/** What the trace knows of the purchase: its day and its price in the main currency at that day. */
export interface TracePurchase {
  day: number | null;
  value: number | null;
}

/**
 * The value line of a property and its marks (`RealEstateDetail.tsx`): the purchase when its day,
 * its price and a rate are known, every estimate at the rates of its own day (one per day, the
 * newest wins) and today, `nowValue` at today's rates. The purchase is a point of the line only
 * while it precedes the first estimate (`valuationTrace`); its mark stays on the line either way
 * and wins where an estimate falls on the same day, the two would overlap. A property without an
 * estimate has no trace.
 */
export function propertyTrace(
  valuations: readonly AssetValuation[],
  purchase: TracePurchase,
  now: number,
  nowValue: number | null,
  convertAt: ConvertAtFn
): PropertyTrace {
  const { valuations: estimates, missing } = convertValuations(
    dailyValuations(
      valuations.map((v) => ({
        id: v.id,
        t: v.valuedAt,
        value: Number(v.value) || 0,
        currency: recordCurrency(v.currency),
        createdAt: v.createdAt,
        note: v.note,
      }))
    ),
    convertAt
  );
  const purchasePoint =
    purchase.day !== null && purchase.value !== null && purchase.value > 0
      ? { t: purchase.day, value: purchase.value }
      : null;

  const points = valuationTrace({
    valuations: estimates,
    purchase: purchasePoint,
    now,
    nowValue: nowValue ?? undefined,
  });
  if (points.length === 0) return { points, events: [], missing };

  const marks = estimates
    .map<TraceEvent>((estimate, index) => ({
      id: estimate.id,
      t: estimate.t,
      type: 'mark',
      kind: 'estimate',
      value: estimate.value,
      note: estimate.note,
      first: index === 0,
    }))
    .filter((mark) => !(purchasePoint !== null && mark.t === purchasePoint.t));
  const events: TraceEvent[] = purchasePoint
    ? [
        {
          id: 'purchase',
          t: purchasePoint.t,
          type: 'buy',
          kind: 'purchase',
          value: purchasePoint.value,
        },
        ...marks,
      ]
    : marks;
  return { points, events, missing };
}

/**
 * Whether the trace reaches two different UTC days. One estimate dated today and no purchase gives
 * a line from this morning to now: a flat stroke across one day with four equal axis labels, which
 * says nothing — such a trace is not drawn.
 */
export function traceSpansDays(points: readonly CostPoint[]): boolean {
  if (points.length < 2) return false;
  const first = utcDayFloor(points[0].t);
  return points.some((point) => utcDayFloor(point.t) !== first);
}

/** One estimate and no purchase on the trace: nothing to read a trend from yet (the desktop's hint). */
export function oneEstimateOnly(events: readonly TraceEvent[]): boolean {
  return events.length === 1 && events[0].kind === 'estimate';
}

/**
 * The change of the market value against the purchase price: the amount and its ratio of the
 * price. `null` when either side has no rate or the purchase price is nothing.
 */
export function appreciationOf(row: PropertyRow): { delta: number; ratio: number } | null {
  if (row.appreciation === null || row.purchase === null || row.purchase <= 0) return null;
  return { delta: row.appreciation, ratio: row.appreciation / row.purchase };
}

export type StatTone = 'gain' | 'loss';

/** An amount in the main currency, with its own as the fallback while it has no rate. */
export interface MoneyPair {
  main: number | null;
  native: NativeAmount;
}

export interface PropertyStatItems {
  /** Hodnota: the market value and its change against the purchase. */
  value: {
    /** The main currency; `null` without a rate (the screen then shows the native amount). */
    amount: number | null;
    native: NativeAmount;
    /** A ratio of the purchase price (0,25 = 25 %), negative for a loss; `null` when unknown. */
    appreciation: number | null;
  };
  /** Vlastní kapitál with its loan to value; `hasLoan` is false for a property without a loan balance. */
  equity: { amount: number | null; ltv: number | null; hasLoan: boolean };
  /** Hrubý výnos: the yearly rent over the value; no yield (and no rent) for a property that is not rented. */
  yield: { hasRent: boolean; ratio: number | null; rent: MoneyPair | null };
  /** Čistý cashflow of a year; the tone of a non-zero result. */
  cashflow: { amount: number | null; tone: StatTone | null };
}

/** The four stats of the detail (`RealEstateDetail.tsx`'s derived numbers), unformatted. */
export function propertyStatItems(row: PropertyRow, balance: YearlyBalance): PropertyStatItems {
  const hasRent = balance.hasRent;
  const cashflow = balance.cashflow;
  return {
    value: {
      amount: row.value,
      native: row.valueNative,
      appreciation: appreciationOf(row)?.ratio ?? null,
    },
    equity: {
      amount: row.equity,
      ltv: row.ltv,
      // A loan total with no rate is a loan with a balance (an amount of nothing needs none)
      hasLoan: row.loans === null || row.loans > 0,
    },
    yield: {
      hasRent,
      ratio: hasRent ? row.grossYield : null,
      rent: hasRent
        ? {
            main: row.rentYear,
            native: {
              amount: row.rentMonthNative.amount * 12,
              currency: row.rentMonthNative.currency,
            },
          }
        : null,
    },
    cashflow: {
      amount: cashflow,
      tone: cashflow === null ? null : cashflow > 0 ? 'gain' : cashflow < 0 ? 'loss' : null,
    },
  };
}

/**
 * The monthly rent: in the main currency (a twelfth of the balance's yearly rent) and in its own,
 * which is what the screen writes while the main one has no rate.
 */
export function monthlyRent(property: RealEstate, balance: YearlyBalance): MoneyPair {
  return {
    main: balance.rent !== null ? balance.rent / 12 : null,
    native: {
      amount: Number(property.monthlyRent) || 0,
      currency: recordCurrency(property.monthlyRentCurrency),
    },
  };
}

export type BalanceKey =
  'rent' | 'recurring' | 'insurance' | 'interest' | 'netIncome' | 'principal' | 'cashflow';

export interface BalanceRow {
  key: BalanceKey;
  /** Signed as the card shows it (costs and the principal negative); `null` when a rate is missing. */
  amount: number | null;
  /** `gain` / `loss` colour a line, `total` the two results. */
  tone?: StatTone | 'total';
}

/** Minus a known amount; zero stays zero (never "−0"). */
const negate = (value: number | null): number | null => (value === null ? null : -value || 0);

/** The lines of the "Roční bilance" card, in the desktop's order and with its signs and colours. */
export function balanceRows(balance: YearlyBalance): BalanceRow[] {
  return [
    {
      key: 'rent',
      amount: balance.rent,
      tone: balance.rent !== null && balance.rent > 0 ? 'gain' : undefined,
    },
    {
      key: 'recurring',
      amount: negate(balance.recurring),
      tone: (balance.recurring ?? 0) > 0 ? 'loss' : undefined,
    },
    {
      key: 'insurance',
      amount: negate(balance.insurance),
      tone: (balance.insurance ?? 0) > 0 ? 'loss' : undefined,
    },
    {
      key: 'interest',
      amount: negate(balance.interest),
      tone: (balance.interest ?? 0) > 0 ? 'loss' : undefined,
    },
    { key: 'netIncome', amount: balance.netIncome, tone: 'total' },
    {
      key: 'principal',
      amount: negate(balance.principal),
      tone: (balance.principal ?? 0) > 0 ? 'loss' : undefined,
    },
    { key: 'cashflow', amount: balance.cashflow, tone: 'total' },
  ];
}

/**
 * The sentence under the card: nothing to balance, what is missing, or the result in words — a
 * month of the cashflow (`monthly`, unsigned) and the net income of a year (`income`).
 */
export type BalanceNote =
  | { kind: 'empty' }
  | { kind: 'missing'; currencies: string[] }
  | { kind: 'costs' | 'earns'; monthly: number; income: number };

export function balanceNote(balance: YearlyBalance): BalanceNote {
  if (!balance.hasRent && !balance.hasRecurring && !balance.hasPayments) return { kind: 'empty' };
  const { cashflow, netIncome } = balance;
  if (cashflow === null || netIncome === null) {
    return { kind: 'missing', currencies: balance.missing };
  }
  return cashflow < 0
    ? { kind: 'costs', monthly: Math.abs(cashflow) / 12, income: netIncome }
    : { kind: 'earns', monthly: cashflow / 12, income: netIncome };
}

/**
 * What Připojit offers: the loans or policies not linked to this property. The backend already
 * leaves out everything linked to a property (linking one that belongs to another would silently
 * move it); a refetch that has not landed yet may still list one that has just been linked here.
 */
export function linkCandidates<T extends { id: string }>(
  available: readonly T[],
  linked: readonly { id: string }[]
): T[] {
  const taken = new Set(linked.map((item) => item.id));
  return available.filter((item) => !taken.has(item.id));
}

/** The premium of a linked policy as its row shows it: one payment (the lump sum of a one-time policy), native. */
export function policyPremium(policy: InsurancePolicy): {
  amount: number;
  currency: string;
  frequency: PaymentFrequency | null;
} {
  const frequency = parsePaymentFrequency(policy.paymentFrequency);
  return frequency === 'one_time'
    ? {
        amount: Number(policy.oneTimePayment) || 0,
        currency: recordCurrency(policy.oneTimePaymentCurrency),
        frequency,
      }
    : {
        amount: Number(policy.regularPayment) || 0,
        currency: recordCurrency(policy.regularPaymentCurrency),
        frequency,
      };
}

/**
 * The currencies the screen names as converted without a rate. Nothing while the current quotes
 * load (a null conversion then is pending, not missing); the amounts at the rates of their own day
 * (the purchase, the costs, the estimates) count once those rates have loaded as well.
 */
export function propertyMissing(args: {
  fxReady: boolean;
  datedReady: boolean;
  /** Currencies of today's amounts (value, rent, loans, the yearly balance). */
  now: readonly string[];
  /** Currencies of the dated amounts. */
  dated: readonly string[];
}): string[] {
  if (!args.fxReady) return [];
  return [...new Set([...args.now, ...(args.datedReady ? args.dated : [])])];
}

/** The currencies of the one-time costs whose day has no rate (an amount of nothing needs none). */
export function costsMissing(
  costs: readonly RealEstateOneTimeCost[],
  convertAt: ConvertAtFn
): string[] {
  const missing = new Set<string>();
  for (const cost of costs) {
    const amount = Number(cost.amount) || 0;
    const currency = recordCurrency(cost.currency);
    if (amount !== 0 && convertAt(amount, currency, cost.date) === null) missing.add(currency);
  }
  return [...missing];
}

/** Today's UTC day as `YYYY-MM-DD` (the revaluation day is a UTC day, ADR 0008). */
export function todayIso(now: Date = new Date()): string {
  return now.toISOString().split('T')[0];
}

/** The UTC-midnight unix second of an ISO day. */
export function isoToUtcDay(iso: string): number {
  const [year, month, day] = iso.split('-').map(Number);
  return Math.floor(Date.UTC(year, month - 1, day) / 1000);
}

export interface RevalueForm {
  /** As typed on the keypad. */
  valueText: string;
  /** The date field: `YYYY-MM-DD`, or empty. */
  dateIso: string;
  note: string;
}

export interface RevalueErrors {
  value?: 'required' | 'invalid';
  date?: 'required' | 'future';
}

/**
 * The revaluation the form makes (the desktop `RevalueDialog`'s rules): an amount of zero or more,
 * a day that is not in the future, an optional note. The amount goes to the backend as decimal
 * text, the day as its UTC-midnight second.
 */
export function revalueData(
  form: RevalueForm,
  args: { assetId: string; currency: string; today: string }
): { ok: true; data: InsertAssetValuation } | { ok: false; errors: RevalueErrors } {
  const errors: RevalueErrors = {};
  const value = parseAmountInput(form.valueText);
  if (form.valueText.trim() === '') errors.value = 'required';
  else if (value === null || value.startsWith('-')) errors.value = 'invalid';
  if (form.dateIso === '') errors.date = 'required';
  else if (form.dateIso > args.today) errors.date = 'future';
  if (errors.value || errors.date || value === null) return { ok: false, errors };
  return {
    ok: true,
    data: {
      assetId: args.assetId,
      value,
      currency: args.currency,
      valuedAt: isoToUtcDay(form.dateIso),
      note: form.note.trim() || null,
    },
  };
}

/** The queries kept under the property's own id: dropped once the screen has left a deleted property. */
export function propertyOwnKeys(id: string): readonly QueryKey[] {
  return [
    ['real-estate', id],
    ['real-estate-valuations', id],
    ['real-estate-costs', id],
    ['real-estate-loans', id],
    ['real-estate-insurances', id],
    ['real-estate-photo-batches', id],
    ['real-estate-documents', id],
  ];
}
