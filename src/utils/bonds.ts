import type { Bond } from '@shared/schema';
import { convertToCzK, type CurrencyCode } from '@shared/currencies';

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

export function bondValueCzk(bond: Bond): number {
  return convertToCzK(bondFaceValue(bond), (bond.currency || 'CZK') as CurrencyCode);
}

export function bondCouponCzk(bond: Bond): number {
  return convertToCzK(bondYearlyCoupon(bond), (bond.currency || 'CZK') as CurrencyCode);
}

/** A bond whose maturity day is before today has been repaid. */
export function isMatured(bond: Bond, todaySec: number): boolean {
  return bond.maturityDate !== null && bond.maturityDate < todaySec;
}

export interface LadderYear {
  year: number;
  /** Face value returned that year, CZK. */
  principal: number;
  /** Coupons still due that year, CZK (yearly coupon assumed on the maturity anniversary). */
  coupons: number;
}

/**
 * Cash coming back per calendar year from today's year to the last maturity:
 * principal in the maturity year, one yearly coupon on each remaining
 * anniversary of the maturity date. Bonds without a maturity pay a coupon
 * every year of the range. Capped at `maxYears` columns.
 */
export function bondLadder(bonds: Bond[], todaySec: number, maxYears = 15): LadderYear[] {
  const live = bonds.filter((b) => !isMatured(b, todaySec));
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
      const coupon = bondCouponCzk(bond);
      if (bond.maturityDate === null) {
        coupons += coupon;
        continue;
      }
      const maturity = new Date(bond.maturityDate * 1000);
      if (maturity.getUTCFullYear() === year) principal += bondValueCzk(bond);
      const anniversary = Date.UTC(year, maturity.getUTCMonth(), maturity.getUTCDate()) / 1000;
      if (anniversary > todaySec && anniversary <= bond.maturityDate) coupons += coupon;
    }
    years.push({ year, principal, coupons });
  }
  return years;
}

/** Years to maturity weighted by CZK face value; bonds without a maturity are left out. */
export function weightedYearsToMaturity(bonds: Bond[], todaySec: number): number | null {
  let weighted = 0;
  let weight = 0;
  for (const bond of bonds) {
    if (bond.maturityDate === null || isMatured(bond, todaySec)) continue;
    const years = (bond.maturityDate - todaySec) / (365.25 * SECONDS_PER_DAY);
    const value = bondValueCzk(bond);
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
