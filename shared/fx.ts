/**
 * Pure foreign-exchange conversion over backend quotes.
 *
 * Quotes are "units of each currency per 1 unit of `reference`" (the backend
 * includes derived codes such as AED/SAR/BGN). The reference itself is never
 * assumed: it is carried in the data and always counts as 1. A missing quote
 * means `null`, never 1:1 and never a hardcoded rate; converting a currency to
 * itself is the identity and needs no quote at all.
 */

export type FxQuotes = { reference: string; units: Record<string, number> };
export type FxDay = { day: number; units: Record<string, number> };
export type FxHistory = { reference: string; days: FxDay[] };

const SECONDS_PER_DAY = 86_400;
/** Historical lookups reach at most this many days before the requested day. */
const MAX_LOOKBACK_DAYS = 10;

/** Trimmed upper-case ISO-style code, or null for empty input. */
export function normalizeCode(code: string | null | undefined): string | null {
  const normalized = (code ?? '').trim().toUpperCase();
  return normalized === '' ? null : normalized;
}

function unitsOf(units: Record<string, number>, reference: string, code: string): number | null {
  const value = code === reference ? 1 : units[code];
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : null;
}

function convertWith(
  units: Record<string, number>,
  reference: string,
  amount: number,
  from: string,
  to: string
): number | null {
  const unitsFrom = unitsOf(units, reference, from);
  const unitsTo = unitsOf(units, reference, to);
  if (unitsFrom === null || unitsTo === null) return null;
  return (amount / unitsFrom) * unitsTo;
}

/** Convert at the current quotes; null when either currency has no quote. */
export function convert(
  q: FxQuotes | undefined,
  amount: number,
  from: string,
  to: string
): number | null {
  const f = normalizeCode(from);
  const t = normalizeCode(to);
  if (f === null || t === null) return null;
  if (f === t) return amount;
  if (!q) return null;
  return convertWith(q.units, normalizeCode(q.reference) ?? q.reference, amount, f, t);
}

/** Daily quotes sorted oldest first. */
export function makeHistory(days: FxDay[], reference: string): FxHistory {
  return { reference, days: [...days].sort((a, b) => a.day - b.day) };
}

/**
 * Convert at the quotes of one common UTC day on or before the timestamp's
 * day, at most ten days back: both currencies must be quoted on that same day.
 */
export function convertOn(
  h: FxHistory | undefined,
  amount: number,
  from: string,
  to: string,
  timestamp: number
): number | null {
  const f = normalizeCode(from);
  const t = normalizeCode(to);
  if (f === null || t === null) return null;
  if (f === t) return amount;
  if (!h) return null;
  const requestedDay = Math.floor(timestamp / SECONDS_PER_DAY) * SECONDS_PER_DAY;
  const earliest = requestedDay - MAX_LOOKBACK_DAYS * SECONDS_PER_DAY;
  const reference = normalizeCode(h.reference) ?? h.reference;
  for (let i = h.days.length - 1; i >= 0; i--) {
    const day = h.days[i];
    if (day.day > requestedDay) continue;
    if (day.day < earliest) break;
    const result = convertWith(day.units, reference, amount, f, t);
    if (result !== null) return result;
  }
  return null;
}

/** Collects the currencies that could not be converted while summing a view. */
export class MissingTracker {
  private readonly codes = new Set<string>();

  /** Pass a conversion result through; a null result records `from` as missing. */
  track(from: string, v: number | null): number | null {
    if (v === null) {
      const code = normalizeCode(from);
      if (code !== null) this.codes.add(code);
    }
    return v;
  }

  get missing(): string[] {
    return [...this.codes];
  }

  get complete(): boolean {
    return this.codes.size === 0;
  }
}
