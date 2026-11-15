/**
 * The account detail's own rules as pure functions (audit D10): what its sub-line names and when
 * its "Průměrně za měsíc" stat has enough to say. The screen words them.
 */

/** What the sub-line under the account's name names; `manual` adds "Vedeno ručně" before them. */
export interface AccountDetailSub {
  parts: string[];
  /** No bank and no imported statement: the account is kept by hand. */
  manual: boolean;
}

/**
 * The parts of the sub-line: the bank, the currency when it is not the main one (the main currency
 * goes without saying, as on the Finance rows), the account number. An account with no bank and
 * no statement is kept by hand, which the sub-line says first. An account that has statements but
 * nothing else to name keeps its currency, so the line is never empty.
 */
export function accountDetailSub(args: {
  institution: string | null | undefined;
  currency: string;
  identifier: string | null | undefined;
  mainCurrency: string;
  /** The account has an imported statement. */
  imported: boolean;
}): AccountDetailSub {
  const institution = args.institution?.trim() || null;
  const identifier = args.identifier?.trim() || null;
  const manual = institution === null && !args.imported;
  const parts = [
    institution,
    args.currency !== args.mainCurrency ? args.currency : null,
    identifier,
  ].filter((part): part is string => part !== null);
  if (parts.length === 0 && !manual) parts.push(args.currency);
  return { parts, manual };
}

/**
 * Whether the average monthly change is worth a stat: a year of records always is; counted from
 * the first record it needs two months at least, since one month's change is no average.
 */
export function hasMonthlyAverage(
  average: { months: number; sinceFirst: boolean } | null
): boolean {
  if (average === null) return false;
  return !average.sinceFirst || average.months >= 2;
}
