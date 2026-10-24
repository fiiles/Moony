import type { Bond } from '@shared/schema';
import { bondCurrency, bondFaceValue, isMatured } from '@/utils/bonds';

const SECONDS_PER_DAY = 86_400;

/** A bond whose maturity is this near (or nearer) wears the "do roka" badge. */
const WITHIN_YEAR_SECONDS = 365 * SECONDS_PER_DAY;

/** One bond of the issues list, with the numbers its row writes (all in the bond's own currency). */
export interface BondRowModel {
  bond: Bond;
  /** Face value × quantity. */
  face: number;
  currency: string;
  /** Yearly coupon rate, percent. */
  rate: number;
  quantity: number;
  /** Face value of one unit. */
  unitFace: number;
  /** The maturity day (UTC, unix seconds); `null` for a bond without one. */
  maturity: number | null;
  badge: 'withinYear' | 'matured' | null;
}

/**
 * The issues list as the Dluhopisy page shows it (spec §8.8): repaid bonds only on request, then
 * by maturity — the nearest first, a bond without a maturity last, equal rows in the order they
 * came in. The badge is "matured" for a repaid bond, "withinYear" when it is repaid within 365
 * days (a maturity today included), nothing otherwise.
 */
export function bondRows(
  bonds: readonly Bond[],
  todaySec: number,
  showMatured: boolean
): BondRowModel[] {
  return bonds
    .filter((bond) => showMatured || !isMatured(bond, todaySec))
    .map<BondRowModel>((bond) => {
      const maturity = bond.maturityDate;
      return {
        bond,
        face: bondFaceValue(bond),
        currency: bondCurrency(bond),
        rate: parseFloat(bond.interestRate || '0') || 0,
        quantity: parseFloat(bond.quantity || '1') || 0,
        unitFace: parseFloat(bond.couponValue || '0') || 0,
        maturity,
        badge:
          maturity === null
            ? null
            : isMatured(bond, todaySec)
              ? 'matured'
              : maturity - todaySec <= WITHIN_YEAR_SECONDS
                ? 'withinYear'
                : null,
      };
    })
    .sort((a, b) =>
      a.maturity === b.maturity
        ? 0
        : a.maturity === null
          ? 1
          : b.maturity === null
            ? -1
            : a.maturity - b.maturity
    );
}
