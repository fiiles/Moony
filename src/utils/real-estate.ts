import type { Loan, RealEstate } from '@shared/schema';
import { MissingTracker, type ConvertAtFn, type ConvertFn } from '@shared/fx';

/** An amount in its own currency (a blank currency is CZK, the legacy default). */
export interface NativeAmount {
  amount: number;
  currency: string;
}

const native = (
  amount: string | number | null | undefined,
  currency: string | null | undefined
): NativeAmount => ({ amount: Number(amount) || 0, currency: currency || 'CZK' });

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
