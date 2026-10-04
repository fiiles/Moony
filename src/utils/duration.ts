/**
 * Calendar months between two unix-second timestamps (UTC), the way people
 * say "držíte 2 roky 7 měsíců": whole months only, a partial month is not
 * counted until its day of month is reached.
 */
export interface MonthDuration {
  years: number;
  months: number;
  totalMonths: number;
}

export function monthsBetween(fromSec: number, toSec: number): MonthDuration {
  if (!Number.isFinite(fromSec) || !Number.isFinite(toSec) || toSec <= fromSec) {
    return { years: 0, months: 0, totalMonths: 0 };
  }
  const from = new Date(fromSec * 1000);
  const to = new Date(toSec * 1000);
  let total =
    (to.getUTCFullYear() - from.getUTCFullYear()) * 12 + (to.getUTCMonth() - from.getUTCMonth());
  if (to.getUTCDate() < from.getUTCDate()) total -= 1;
  total = Math.max(0, total);
  return { years: Math.floor(total / 12), months: total % 12, totalMonths: total };
}
