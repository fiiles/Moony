import type { QueryKey } from '@tanstack/react-query';
import type {
  AssetValuation,
  InsertRealEstateOneTimeCost,
  InsurancePolicy,
  RealEstate,
  RealEstateOneTimeCost,
  RealEstatePhotoBatch,
  RecurringCost,
} from '@shared/schema';
import { parsePaymentFrequency, type PaymentFrequency } from '@shared/calculations';
import { recordCurrency } from '@shared/currencies';
import type { ConvertAtFn } from '@shared/fx';
import type { Formatters } from '@/lib/format';
import { utcDayFloor } from '@/utils/chart-axis';
import type { ChartEvent } from '@/utils/chart-scale';
import {
  yearlyFactor,
  type PropertyRow,
  type YearlyBalance,
  type NativeAmount,
} from '@/utils/real-estate';
import { convertValuations, dailyValuations, valuationTrace } from '@/utils/valuation-trace';
import { parseAmountInput } from './amount-input';
import type { CostPoint } from './position-series';

/**
 * The property detail's numbers as pure functions (spec §8.8 "Nemovitost"): the valuation trace,
 * the four stats, the yearly balance card, the link candidates, the costs with their form, and the
 * batch a photo taken today joins (the revaluation form is `revalue.ts`, shared with the other
 * asset). They are the desktop page's rules (`pages/RealEstateDetail.tsx`) without its strings and
 * formatting; the phone screen words them.
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

/**
 * The batch a photo taken today joins: the one of today's UTC day (a batch is a day, ADR 0008), so
 * all the photos of one day end up together. `today` is today's UTC-midnight second or any moment
 * of the day; `null` while no batch has that day, and the first photo then starts one. When the
 * desktop has made two batches for the day, the first (the list comes newest day first) takes it.
 */
export function todayBatch<T extends { photoDate: number }>(
  batches: readonly T[],
  today: number
): T | null {
  const dayStart = utcDayFloor(today);
  return batches.find((batch) => utcDayFloor(batch.photoDate) === dayStart) ?? null;
}

/** One photo of the gallery with what the viewer and the delete need of its batch. */
export interface GalleryPhoto {
  id: string;
  batchId: string;
  /** The batch's day (UTC midnight). */
  day: number;
  /** The batch's note, as typed on the computer. */
  description: string | null;
  /** The only photo of its batch: deleting it deletes the batch, so none is left empty. */
  last: boolean;
}

/**
 * The photos of the gallery in the order the grid draws them: the newest batch's first (a later-
 * created batch first within a day), each batch's photos as stored. A batch without photos
 * has nothing to draw.
 */
export function galleryPhotos(batches: readonly RealEstatePhotoBatch[]): GalleryPhoto[] {
  return [...batches]
    .sort((a, b) => b.photoDate - a.photoDate || b.createdAt - a.createdAt)
    .flatMap((batch) =>
      batch.photos.map<GalleryPhoto>((photo) => ({
        id: photo.id,
        batchId: batch.id,
        day: batch.photoDate,
        description: batch.description,
        last: batch.photos.length === 1,
      }))
    );
}

/**
 * What deleting a photo does, decided on the batches as they are *now*: the whole batch when the
 * photo is still its only one (none is left empty), the photo alone when the batch has other
 * photos — an upload into today's batch may have landed after the photo was opened, and a decision
 * made when it was tapped would delete those with it. `gone` when no batch holds the photo (it was
 * deleted elsewhere), so there is nothing left to delete.
 */
export type PhotoDeletion =
  { kind: 'batch'; batchId: string } | { kind: 'photo'; photoId: string } | { kind: 'gone' };

export function photoDeletion(
  batches: readonly RealEstatePhotoBatch[],
  photoId: string
): PhotoDeletion {
  const batch = batches.find((candidate) => candidate.photos.some((photo) => photo.id === photoId));
  if (!batch) return { kind: 'gone' };
  return batch.photos.length === 1
    ? { kind: 'batch', batchId: batch.id }
    : { kind: 'photo', photoId };
}

/**
 * The costs' text: the phone's `useMoneyText()` as `money` and `fmt.day` as `day`.
 */
export type CostText = Pick<Formatters, 'money' | 'day'>;

/** One recurring cost of the property as its row shows it, in the cost's own currency. */
export interface RecurringCostRow {
  /** The place in `property.recurringCosts`: what `withoutRecurring` removes. */
  index: number;
  name: string;
  /** `monthly`, `quarterly` or `yearly`, lower case (the desktop's `detail.frequency.*`). */
  frequency: string;
  /** How many of its payments make a year (`yearlyFactor`: 12, 4, else 1). */
  perYear: number;
  /** One payment. */
  amount: string;
  /** The payments of a year. */
  yearly: string;
}

/** One one-time cost as its row shows it. */
export interface OneTimeCostRow {
  id: string;
  name: string;
  description: string | null;
  /** The day it was paid. */
  day: string;
  amount: string;
  /** The stored cost: what the edit sheet starts from. */
  cost: RealEstateOneTimeCost;
}

/** Whole amounts: the cost rows show no decimals. */
const WHOLE = { decimals: 0 } as const;

/**
 * The rows of "Náklady" (`pages/RealEstateDetail.tsx`'s two tables): the recurring costs in their
 * order with the amount of a year (`yearlyFactor`), and the one-time costs newest day first (the
 * later-saved first within a day). Every amount is in its own currency — the native value is the
 * authoritative one — and goes through `text.money`, so nothing needs a rate.
 */
export function costRows(
  property: RealEstate,
  costs: readonly RealEstateOneTimeCost[],
  text: CostText
): { recurring: RecurringCostRow[]; oneTime: OneTimeCostRow[] } {
  const recurring = (property.recurringCosts ?? []).map<RecurringCostRow>((cost, index) => {
    const amount = Number(cost.amount) || 0;
    const currency = recordCurrency(cost.currency);
    const perYear = yearlyFactor(cost.frequency);
    return {
      index,
      name: cost.name,
      frequency: (cost.frequency || '').toLowerCase(),
      perYear,
      amount: text.money(amount, currency, WHOLE),
      yearly: text.money(amount * perYear, currency, WHOLE),
    };
  });
  const oneTime = [...costs]
    .sort((a, b) => b.date - a.date || b.createdAt - a.createdAt)
    .map<OneTimeCostRow>((cost) => ({
      id: cost.id,
      name: cost.name,
      description: cost.description,
      day: text.day(cost.date),
      amount: text.money(Number(cost.amount) || 0, recordCurrency(cost.currency), WHOLE),
      cost,
    }));
  return { recurring, oneTime };
}

/** The costs without the recurring one at `index`; a list it has no such place in is unchanged. */
export function withoutRecurring(costs: readonly RecurringCost[], index: number): RecurringCost[] {
  return costs.filter((_, place) => place !== index);
}

export type CostKind = 'recurring' | 'oneTime';
export type CostFrequency = 'monthly' | 'quarterly' | 'yearly';

/** What the cost sheet holds while it is open: the one form of both kinds. */
export interface CostForm {
  realEstateId: string;
  kind: CostKind;
  name: string;
  /** As typed on the keypad. */
  amountText: string;
  /** A recurring cost's. */
  frequency: CostFrequency;
  /** A one-time cost's day: `YYYY-MM-DD`, or empty. */
  dateIso: string;
  /** A one-time cost's note. */
  description: string;
  /** The stored day (`YYYY-MM-DD`) of an edited cost, which it may keep even when it is ahead; `null` for a new cost. */
  originalDateIso: string | null;
}

export interface CostErrors {
  name?: 'required';
  amount?: 'required' | 'invalid' | 'notPositive';
  /** A recurring cost has no day. */
  date?: 'required' | 'future';
}

/** The amount of a cost: a positive number as decimal text; why not, else. */
function costAmount(
  text: string
): { amount: string } | { error: 'required' | 'invalid' | 'notPositive' } {
  if (text.trim() === '') return { error: 'required' };
  const parsed = parseAmountInput(text);
  const value = Number(parsed);
  if (parsed === null || !Number.isFinite(value)) return { error: 'invalid' };
  return value > 0 ? { amount: parsed } : { error: 'notPositive' };
}

/**
 * The latest day a cost's day may be (`YYYY-MM-DD`): today — a cost is a payment that has been made,
 * and a day ahead has no rate yet — or, for an edited cost that the computer dated ahead, that day:
 * it may keep it, though not move it further out. What the date field's `max` is.
 */
export function latestCostDay(form: Pick<CostForm, 'originalDateIso'>, today: string): string {
  return form.originalDateIso !== null && form.originalDateIso > today
    ? form.originalDateIso
    : today;
}

/**
 * What stops a cost form from being saved. A cost is a name and a positive amount (the desktop's
 * recurring dialog asks the same), and a one-time cost
 * also the day it was paid, up to `latestCostDay`. `today` is `YYYY-MM-DD`.
 */
export function costErrors(form: CostForm, today: string): CostErrors {
  const errors: CostErrors = {};
  if (form.name.trim() === '') errors.name = 'required';
  const amount = costAmount(form.amountText);
  if ('error' in amount) errors.amount = amount.error;
  if (form.kind === 'oneTime') {
    if (form.dateIso === '') errors.date = 'required';
    else if (form.dateIso > latestCostDay(form, today)) errors.date = 'future';
  }
  return errors;
}

/**
 * The one-time cost a form makes, in `currency` (the property's, or the cost's own when it is
 * edited): the amount as decimal text, the day as its UTC-midnight second (a number the backend
 * keeps, never a `Date`), the trimmed name and note. `null` for a form with no name, no positive
 * amount or no day; the sheet shows why (`costErrors`) and never calls the backend with it.
 */
export function costInput(form: CostForm, currency: string): InsertRealEstateOneTimeCost | null {
  const name = form.name.trim();
  const amount = costAmount(form.amountText);
  if (name === '' || 'error' in amount || form.dateIso === '') return null;
  return {
    realEstateId: form.realEstateId,
    name,
    description: form.description.trim() || undefined,
    amount: amount.amount,
    currency,
    date: isoToUtcDay(form.dateIso),
  };
}

/** The recurring cost a form makes, in `currency`; `null` for a form with no name or no positive amount. */
export function recurringInput(form: CostForm, currency: string): RecurringCost | null {
  const name = form.name.trim();
  const amount = costAmount(form.amountText);
  if (name === '' || 'error' in amount) return null;
  return { name, amount: Number(amount.amount), frequency: form.frequency, currency };
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
