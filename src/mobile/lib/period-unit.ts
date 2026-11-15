/** The period an amount is for, as `mobile:period.*` writes it next to the amount. */
export type PeriodUnit = 'month' | 'quarter' | 'halfYear' | 'year';

const UNITS: Readonly<Record<string, PeriodUnit>> = {
  monthly: 'month',
  quarterly: 'quarter',
  semi_annually: 'halfYear',
  annually: 'year',
  yearly: 'year',
};

/**
 * The unit of a premium's or a cost's frequency (README §13, "Dates and periods", audit
 * 2026-11-15 C6): "2 270 Kč / měs.", "11 216 Kč / rok" stand next to the amount, never as a word in
 * another column. A one-time payment and an unknown frequency have none: the amount stands alone.
 */
export function periodUnit(frequency: string | null | undefined): PeriodUnit | null {
  return (frequency && UNITS[frequency]) || null;
}
