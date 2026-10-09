import type { Bond } from '@shared/schema';
import { MissingTracker, type ConvertFn } from '@shared/fx';
import { recordCurrency } from '@shared/currencies';

const SECONDS_PER_DAY = 86_400;

/** Face value × quantity of one bond in its own currency. */
export function bondFaceValue(bond: Bond): number {
  const value = parseFloat(bond.couponValue || '0') || 0;
  const quantity = parseFloat(bond.quantity || '1') || 0;
  return value * quantity;
}

/** Yearly coupon of one bond in its own currency (face value × rate). */
export function bondYearlyCoupon(bond: Bond): number {
  const rate = parseFloat(bond.interestRate || '0') || 0;
  return (bondFaceValue(bond) * rate) / 100;
}

/** The bond's currency (legacy rows without one are CZK). */
export function bondCurrency(bond: Bond): string {
  return recordCurrency(bond.currency);
}

/** Face value × quantity in the main currency; null without a rate. */
export function bondValue(bond: Bond, convert: ConvertFn): number | null {
  return convert(bondFaceValue(bond), bondCurrency(bond));
}

/** Yearly coupon in the main currency; null without a rate. */
export function bondCoupon(bond: Bond, convert: ConvertFn): number | null {
  return convert(bondYearlyCoupon(bond), bondCurrency(bond));
}

/** A bond whose maturity day is before today has been repaid. */
export function isMatured(bond: Bond, todaySec: number): boolean {
  return bond.maturityDate !== null && bond.maturityDate < todaySec;
}

export interface LadderYear {
  year: number;
  /** Face value returned that year, main currency. */
  principal: number;
  /** Coupons still due that year, main currency (yearly coupon assumed on the maturity anniversary). */
  coupons: number;
}

/**
 * Cash coming back per calendar year from today's year to the last maturity:
 * principal in the maturity year, one yearly coupon on each remaining
 * anniversary of the maturity date. Bonds without a maturity pay a coupon
 * every year of the range. Capped at `maxYears` columns. Bonds without a rate are left
 * out (the summary reports their currencies).
 */
export function bondLadder(
  bonds: Bond[],
  todaySec: number,
  convert: ConvertFn,
  maxYears = 15
): LadderYear[] {
  const live = bonds.filter((b) => !isMatured(b, todaySec) && bondValue(b, convert) !== null);
  if (live.length === 0) return [];
  const today = new Date(todaySec * 1000);
  const startYear = today.getUTCFullYear();
  const lastMaturity = live.reduce<number | null>(
    (max, b) =>
      b.maturityDate !== null && (max === null || b.maturityDate > max) ? b.maturityDate : max,
    null
  );
  const endYear =
    lastMaturity === null ? startYear : new Date(lastMaturity * 1000).getUTCFullYear();
  const years: LadderYear[] = [];
  for (let year = startYear; year <= Math.min(endYear, startYear + maxYears - 1); year++) {
    let principal = 0;
    let coupons = 0;
    for (const bond of live) {
      const coupon = bondCoupon(bond, convert) ?? 0;
      if (bond.maturityDate === null) {
        coupons += coupon;
        continue;
      }
      const maturity = new Date(bond.maturityDate * 1000);
      if (maturity.getUTCFullYear() === year) principal += bondValue(bond, convert) ?? 0;
      const anniversary = Date.UTC(year, maturity.getUTCMonth(), maturity.getUTCDate()) / 1000;
      if (anniversary > todaySec && anniversary <= bond.maturityDate) coupons += coupon;
    }
    years.push({ year, principal, coupons });
  }
  return years;
}

/**
 * Years to maturity weighted by the face value in the main currency; bonds without a
 * maturity or without a rate are left out.
 */
export function weightedYearsToMaturity(
  bonds: Bond[],
  todaySec: number,
  convert: ConvertFn
): number | null {
  let weighted = 0;
  let weight = 0;
  for (const bond of bonds) {
    if (bond.maturityDate === null || isMatured(bond, todaySec)) continue;
    const years = (bond.maturityDate - todaySec) / (365.25 * SECONDS_PER_DAY);
    const value = bondValue(bond, convert);
    if (value === null) continue;
    weighted += years * value;
    weight += value;
  }
  return weight > 0 ? weighted / weight : null;
}

/** The live bond that matures first, if any has a maturity. */
export function nearestMaturity(bonds: Bond[], todaySec: number): Bond | null {
  return bonds.reduce<Bond | null>((best, bond) => {
    if (bond.maturityDate === null || isMatured(bond, todaySec)) return best;
    return best === null || bond.maturityDate < (best.maturityDate ?? Infinity) ? bond : best;
  }, null);
}

export interface BondsSummary {
  /** Face value of the live bonds with a rate, main currency. */
  totalValue: number;
  /** Coupon rate weighted by the face value in the main currency, percent. */
  averageYield: number;
  /** Yearly coupons of the live bonds with a rate, main currency. */
  projectedYearlyIncome: number;
  liveCount: number;
  maturedCount: number;
  currencies: string[];
  nearest: Bond | null;
  /** What the nearest maturity returns, main currency; null without a rate. */
  nearestValue: number | null;
  weightedYears: number | null;
  lastMaturity: number | null;
  /** Currencies of live bonds left out of the totals for want of a rate. */
  missing: string[];
}

/** The list page's numbers over the live bonds, in the main currency. */
export function bondsSummary(bonds: Bond[], convert: ConvertFn, todaySec: number): BondsSummary {
  const live = bonds.filter((b) => !isMatured(b, todaySec));
  const tracker = new MissingTracker();
  let totalValue = 0;
  let weightedRate = 0;
  let income = 0;
  for (const bond of live) {
    const value = tracker.track(bondCurrency(bond), bondValue(bond, convert));
    if (value === null) continue;
    totalValue += value;
    weightedRate += value * (parseFloat(bond.interestRate || '0') || 0);
    income += bondCoupon(bond, convert) ?? 0;
  }
  const nearest = nearestMaturity(live, todaySec);
  return {
    totalValue,
    averageYield: totalValue > 0 ? weightedRate / totalValue : 0,
    projectedYearlyIncome: income,
    liveCount: live.length,
    maturedCount: bonds.length - live.length,
    currencies: Array.from(new Set(live.map(bondCurrency))),
    nearest,
    nearestValue: nearest ? bondValue(nearest, convert) : null,
    weightedYears: weightedYearsToMaturity(live, todaySec, convert),
    lastMaturity: live.reduce<number | null>(
      (max, b) =>
        b.maturityDate !== null && (max === null || b.maturityDate > max) ? b.maturityDate : max,
      null
    ),
    missing: tracker.missing,
  };
}
