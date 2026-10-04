import type { SavingsAccountZone } from '@shared/schema';

/** The part of a stored zone the interest maths needs. */
export type ZoneTier = Pick<SavingsAccountZone, 'fromAmount' | 'toAmount' | 'interestRate'>;

/**
 * Upper bound of a zone, or `null` when it is unlimited. A missing, empty or
 * `0` bound is unlimited — a real band always ends above its start, so `0` can
 * only be the legacy "no limit" encoding.
 */
export function zoneUpperBound(zone: Pick<SavingsAccountZone, 'toAmount'>): number | null {
  const raw = (zone.toAmount ?? '').trim();
  if (raw === '') return null;
  const to = parseFloat(raw);
  return Number.isFinite(to) && to !== 0 ? to : null;
}

/**
 * Yearly interest for a balance over progressive interest zones (tiers).
 *
 * This is a display mirror of `services/interest_tiers.rs::yearly_interest`
 *: both are checked against `shared/fixtures/interest-tiers.json`.
 * A zone is a band `[fromAmount, toAmount]` of the balance and only the part of
 * the balance inside the band earns its rate. Bands need not be contiguous
 * (balance in a gap earns nothing for the gap) or sorted.
 *
 * @returns The yearly interest amount (not the rate percentage)
 */
export function calculateZonedInterest(balance: number, zones: readonly ZoneTier[]): number {
  if (!zones || zones.length === 0 || balance <= 0) return 0;

  let totalInterest = 0;
  for (const zone of zones) {
    const zoneFrom = parseFloat(zone.fromAmount || '0');
    const zoneRate = parseFloat(zone.interestRate || '0');
    const upper = zoneUpperBound(zone);

    const amountInZone = (upper === null ? balance : Math.min(balance, upper)) - zoneFrom;
    if (amountInZone <= 0) continue;

    totalInterest += (amountInZone * zoneRate) / 100;
  }
  return totalInterest;
}

/**
 * Effective interest rate (percent per year) of a balance over progressive
 * zones: the yearly interest as a share of the balance.
 */
export function calculateEffectiveRate(balance: number, zones: readonly ZoneTier[]): number {
  if (!zones || zones.length === 0 || balance <= 0) return 0;
  return (calculateZonedInterest(balance, zones) / balance) * 100;
}
