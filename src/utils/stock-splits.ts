import type { StockSplit, StockSplitVerdict } from '@shared/schema';

/** "10:1": new shares for old ones, as the banner, dialog and toasts write a split. */
export function ratioText(numerator: string, denominator: string): string {
  return `${numerator}:${denominator}`;
}

/**
 * The applied split a transaction entered on `day` (UTC midnight) falls before (spec D20), so
 * its quantity and price must already be in today's units. With several, the latest one: the
 * hint names the split the user is most likely to recognise. Null when none follows the day.
 */
export function preSplitHint(splits: readonly StockSplit[], day: number): StockSplit | null {
  let hint: StockSplit | null = null;
  for (const split of splits) {
    if (split.status !== 'applied' || day >= split.effectiveDay) continue;
    if (hint === null || split.effectiveDay > hint.effectiveDay) hint = split;
  }
  return hint;
}

/**
 * The banner's primary action for a verdict (spec D9): rewrite transactions that still look
 * pre-split, keep ones that already look converted, emphasise neither when Moony cannot tell.
 */
export function defaultSplitChoice(verdict: StockSplitVerdict): 'apply' | 'dismiss' | null {
  if (verdict === 'unadjusted') return 'apply';
  if (verdict === 'adjusted') return 'dismiss';
  return null;
}

/**
 * The UTC midnight (unix seconds) of a `yyyy-mm-dd` date input value, or null when the value is
 * empty or not a real calendar day.
 */
export function isoDayToUnix(value: string | undefined): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value ?? '');
  if (!match) return null;
  const [year, month, day] = match.slice(1).map(Number);
  const date = new Date(Date.UTC(year, month - 1, day));
  if (date.getUTCMonth() !== month - 1 || date.getUTCDate() !== day) return null;
  return date.getTime() / 1000;
}
