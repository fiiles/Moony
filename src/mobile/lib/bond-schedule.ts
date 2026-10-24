import type { Bond } from '@shared/schema';
import { bondCurrency, bondFaceValue, bondYearlyCoupon } from '@/utils/bonds';

/** One payment of a bond to come: a yearly coupon, or the maturity (the principal with the last coupon). */
export interface BondPayment {
  /** The day it is due, a UTC day as unix seconds (ADR 0008). */
  t: number;
  bond: Bond;
  kind: 'coupon' | 'maturity';
  /** In the bond's own currency: native amounts are authoritative (ADR 0012), no conversion here. */
  amount: number;
  currency: string;
}

/** Days in a month (`month` is 0-based, as `Date` counts it). */
const daysInMonth = (year: number, month: number): number =>
  new Date(Date.UTC(year, month + 1, 0)).getUTCDate();

/** The `date` of a month as a UTC day in seconds; a date the month lacks falls on its last day. */
const clampedDay = (year: number, month: number, date: number): number =>
  Date.UTC(year, month, Math.min(date, daysInMonth(year, month))) / 1000;

/** The anniversary of a maturity day in `year`: 29 February is the 28th in a year without one. */
function anniversary(maturitySec: number, year: number): number {
  const maturity = new Date(maturitySec * 1000);
  return clampedDay(year, maturity.getUTCMonth(), maturity.getUTCDate());
}

/** `months` after a UTC day, the end of a month respected (30 November + 3 months is 28 February). */
function addMonths(sec: number, months: number): number {
  const from = new Date(sec * 1000);
  const target = new Date(Date.UTC(from.getUTCFullYear(), from.getUTCMonth() + months, 1));
  return clampedDay(target.getUTCFullYear(), target.getUTCMonth(), from.getUTCDate());
}

/**
 * The coupons and maturities due from `todaySec` (the day itself included) for `months` months
 * (spec §8.8, ruling 5): each coupon sits on the anniversary of the maturity day — the desktop
 * ladder's assumption, there is no coupon calendar in the data — and the principal comes with the
 * last coupon on the maturity day itself. A bond without a maturity has no day to show and is
 * left out, as is a repaid one; a rate-0 bond has no coupons but its maturity is still a payment
 * (the principal comes back). The window is half-open, so a yearly coupon due today and again in
 * a year is listed once. Amounts are native; the list is by day, a tie in the order the bonds
 * came in.
 */
export function upcomingBondPayments(
  bonds: readonly Bond[],
  todaySec: number,
  months = 12
): BondPayment[] {
  const end = addMonths(todaySec, months);
  const firstYear = new Date(todaySec * 1000).getUTCFullYear();
  const payments: BondPayment[] = [];

  for (const bond of bonds) {
    const maturity = bond.maturityDate;
    if (maturity === null || maturity < todaySec) continue;
    const currency = bondCurrency(bond);
    const coupon = bondYearlyCoupon(bond);
    const lastYear = new Date(maturity * 1000).getUTCFullYear();

    for (let year = firstYear; year <= lastYear; year++) {
      // The maturity year's anniversary is the maturity itself, whatever its time of day
      const t = year === lastYear ? maturity : anniversary(maturity, year);
      if (t < todaySec) continue;
      if (t >= end) break;
      if (year === lastYear) {
        payments.push({
          t,
          bond,
          kind: 'maturity',
          amount: bondFaceValue(bond) + coupon,
          currency,
        });
      } else if (coupon > 0) {
        payments.push({ t, bond, kind: 'coupon', amount: coupon, currency });
      }
    }
  }

  // `sort` is stable: payments of one day stay in the order the bonds came in
  return payments.sort((a, b) => a.t - b.t);
}
