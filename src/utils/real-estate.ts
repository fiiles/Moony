import type { InsurancePolicy, Loan, LoanSchedule, RealEstate } from '@shared/schema';
import { annualizeAmount } from '@shared/calculations';
import { MissingTracker, type ConvertAtFn, type ConvertFn } from '@shared/fx';
import { recordCurrency } from '@shared/currencies';

const DAY = 86_400;

/** An amount in its own currency (a blank currency is CZK, the legacy default). */
export interface NativeAmount {
  amount: number;
  currency: string;
}

const native = (
  amount: string | number | null | undefined,
  currency: string | null | undefined
): NativeAmount => ({ amount: Number(amount) || 0, currency: recordCurrency(currency) });

/**
 * A property with the list's derived numbers: native amounts plus their main-currency
 * values (`null` when that value cannot be converted).
 */
export interface PropertyRow {
  property: RealEstate;
  valueNative: NativeAmount;
  /** Market value at the current quotes. */
  value: number | null;
  purchaseNative: NativeAmount;
  /** Purchase price at the purchase day's rates (the creation day while the date is unknown). */
  purchase: number | null;
  appreciation: number | null;
  rentMonthNative: NativeAmount;
  /** Yearly rent at the current quotes. */
  rentYear: number | null;
  grossYield: number | null;
  /** Outstanding balance of the linked loans at the current quotes; null when any has no rate. */
  loans: number | null;
  equity: number | null;
  ltv: number | null;
  personal: boolean;
  /** Personal residence left out of net worth by the profile setting. */
  excluded: boolean;
  /** Currencies of today's amounts (value, rent, loans) without a rate. */
  missing: string[];
  /** The purchase currency when the purchase day has no rate. */
  missingPurchase: string[];
}

export function propertyRow(
  property: RealEstate,
  loans: readonly Loan[],
  excludePersonal: boolean,
  convert: ConvertFn,
  convertAt: ConvertAtFn
): PropertyRow {
  const tracker = new MissingTracker();
  const today = (money: NativeAmount) =>
    tracker.track(money.currency, money.amount === 0 ? 0 : convert(money.amount, money.currency));
  const valueNative = native(property.marketPrice, property.marketPriceCurrency);
  const purchaseNative = native(property.purchasePrice, property.purchasePriceCurrency);
  const rentMonthNative = native(property.monthlyRent, property.monthlyRentCurrency);
  const value = today(valueNative);
  const purchase =
    purchaseNative.amount === 0
      ? 0
      : convertAt(
          purchaseNative.amount,
          purchaseNative.currency,
          property.purchaseDate ?? property.createdAt
        );
  const rentYear = today({ ...rentMonthNative, amount: rentMonthNative.amount * 12 });
  const loanValues = loans.map((l) => today(native(l.outstandingBalance, l.currency)));
  const loansTotal = loanValues.some((v) => v === null)
    ? null
    : loanValues.reduce<number>((s, v) => s + (v ?? 0), 0);
  const equity = value !== null && loansTotal !== null ? value - loansTotal : null;
  const personal = property.type === 'personal';
  return {
    property,
    valueNative,
    value,
    purchaseNative,
    purchase,
    appreciation: value !== null && purchase !== null ? value - purchase : null,
    rentMonthNative,
    rentYear,
    grossYield: value !== null && rentYear !== null ? (value > 0 ? rentYear / value : 0) : null,
    loans: loansTotal,
    equity,
    ltv:
      equity !== null && value !== null && loansTotal !== null
        ? value > 0
          ? loansTotal / value
          : 0
        : null,
    personal,
    excluded: personal && excludePersonal,
    missing: tracker.missing,
    missingPurchase: purchase === null ? [purchaseNative.currency] : [],
  };
}

export interface PropertiesSummary {
  /** Market value of the counted properties with a rate. */
  value: number;
  /** Purchase prices and appreciation of the properties where both sides are known. */
  purchase: number;
  appreciation: number;
  /** Yearly rent with a rate, and the value of those rented properties. */
  rent: number;
  rented: number;
  /** Equity, its value and its loans over the properties where all three are known. */
  equity: number;
  equityValue: number;
  loans: number;
  /** Market value of the personal residences (counted or not). */
  personalValue: number;
  /** Currencies of today's amounts without a rate (counted properties). */
  missing: string[];
  /** Purchase currencies whose purchase day has no rate (counted properties). */
  missingPurchase: string[];
}

/** Totals of the counted properties: every known amount summed, pairs only where both are known. */
export function propertiesSummary(rows: readonly PropertyRow[]): PropertiesSummary {
  const counted = rows.filter((r) => !r.excluded);
  const sum = (values: (number | null)[]) => values.reduce<number>((s, v) => s + (v ?? 0), 0);
  const withAppreciation = counted.filter((r) => r.appreciation !== null);
  const withEquity = counted.filter((r) => r.equity !== null);
  const rented = counted.filter((r) => r.rentYear !== null && r.rentYear > 0 && r.value !== null);
  return {
    value: sum(counted.map((r) => r.value)),
    purchase: sum(withAppreciation.map((r) => r.purchase)),
    appreciation: sum(withAppreciation.map((r) => r.appreciation)),
    rent: sum(counted.map((r) => r.rentYear)),
    rented: sum(rented.map((r) => r.value)),
    equity: sum(withEquity.map((r) => r.equity)),
    equityValue: sum(withEquity.map((r) => r.value)),
    loans: sum(withEquity.map((r) => r.loans)),
    personalValue: sum(rows.filter((r) => r.personal).map((r) => r.value)),
    missing: [...new Set(counted.flatMap((r) => r.missing))],
    missingPurchase: [...new Set(counted.flatMap((r) => r.missingPurchase))],
  };
}

/** Yearly factor of a recurring real-estate cost (`monthly` / `quarterly` / `yearly`). */
export function yearlyFactor(frequency: string | undefined): number {
  switch ((frequency || '').toLowerCase()) {
    case 'monthly':
      return 12;
    case 'quarterly':
      return 4;
    default:
      return 1;
  }
}

/**
 * The property's yearly balance in the main currency. Every line is null when any of its
 * amounts has no rate (never counted as 0); the `has*` flags come from the native amounts,
 * so a line without a rate still exists.
 */
export interface YearlyBalance {
  rent: number | null;
  recurring: number | null;
  insurance: number | null;
  /** The linked loans' payments of a year. */
  payments: number | null;
  /** Interest and principal in the payments due within the next 12 months. */
  interest: number | null;
  principal: number | null;
  /** rent − recurring − insurance − interest; null when any of them is unknown. */
  netIncome: number | null;
  /** netIncome − principal. */
  cashflow: number | null;
  hasRent: boolean;
  hasRecurring: boolean;
  hasPayments: boolean;
  hasPrincipal: boolean;
  /** Currencies of the amounts without a rate. */
  missing: string[];
}

/** Sum of converted parts; null when any part is unknown. */
function sumKnown(values: readonly (number | null)[]): number | null {
  let total = 0;
  for (const value of values) {
    if (value === null) return null;
    total += value;
  }
  return total;
}

export function yearlyBalance(
  property: RealEstate,
  loans: readonly Loan[],
  schedules: readonly (LoanSchedule | undefined)[],
  insurances: readonly InsurancePolicy[],
  today: number,
  convert: ConvertFn
): YearlyBalance {
  const tracker = new MissingTracker();
  const toMain = (amount: number, currency: string | null | undefined) => {
    const code = recordCurrency(currency);
    return tracker.track(code, amount === 0 ? 0 : convert(amount, code));
  };
  const rentMonth = Number(property.monthlyRent) || 0;
  const rent = toMain(rentMonth * 12, property.monthlyRentCurrency);
  const recurringNative = (property.recurringCosts ?? []).map((c) => ({
    amount: (Number(c.amount) || 0) * yearlyFactor(c.frequency),
    currency: c.currency,
  }));
  const recurring = sumKnown(recurringNative.map((c) => toMain(c.amount, c.currency)));
  const insurance = sumKnown(
    insurances.map((p) =>
      toMain(
        annualizeAmount(Number(p.regularPayment) || 0, p.paymentFrequency),
        p.regularPaymentCurrency
      )
    )
  );
  const paymentsNative = loans.map((l) => (Number(l.monthlyPayment) || 0) * 12);
  const payments = sumKnown(loans.map((l, i) => toMain(paymentsNative[i], l.currency)));
  // Next 12 payments split by the schedules; a missing schedule falls back to rate × balance
  const horizon = today + 365 * DAY;
  const split = loans.map((loan, i) => {
    const schedule = schedules[i];
    if (schedule) {
      const next = schedule.rows.filter((r) => r.dueDay > today && r.dueDay <= horizon);
      return {
        interest: next.reduce((s, r) => s + Number(r.interest), 0),
        principal: next.reduce((s, r) => s + Number(r.principalPart), 0),
      };
    }
    const interest = (Number(loan.outstandingBalance) * (Number(loan.interestRate) || 0)) / 100;
    return { interest, principal: paymentsNative[i] - interest };
  });
  const interest = sumKnown(split.map((p, i) => toMain(p.interest, loans[i].currency)));
  const principal = sumKnown(split.map((p, i) => toMain(p.principal, loans[i].currency)));
  const netIncome =
    rent !== null && recurring !== null && insurance !== null && interest !== null
      ? rent - recurring - insurance - interest
      : null;
  return {
    rent,
    recurring,
    insurance,
    payments,
    interest,
    principal,
    netIncome,
    cashflow: netIncome !== null && principal !== null ? netIncome - principal : null,
    hasRent: rentMonth > 0,
    hasRecurring: recurringNative.some((c) => c.amount > 0),
    hasPayments: paymentsNative.some((p) => p > 0),
    hasPrincipal: split.some((p) => p.principal > 0),
    missing: tracker.missing,
  };
}
