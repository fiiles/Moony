/**
 * Payment frequency shared by insurance, cashflow and forms.
 *
 * Mirrors `src-tauri/src/models/frequency.rs`: the canonical values are what
 * the backend validates and stores (migration 010 normalized legacy
 * spellings); `paymentsPerYear` is lenient about legacy spellings so that
 * JSON blobs written before the migration (real-estate recurring costs use
 * "yearly") still annualize correctly instead of silently counting as 0 or ×12
 *.
 */

export const PAYMENT_FREQUENCIES = [
  'monthly',
  'quarterly',
  'semi_annually',
  'annually',
  'one_time',
] as const;

export type PaymentFrequency = (typeof PAYMENT_FREQUENCIES)[number];

const LEGACY_ALIASES: Record<string, PaymentFrequency> = {
  'semi-annually': 'semi_annually',
  semiannually: 'semi_annually',
  semi_annual: 'semi_annually',
  half_yearly: 'semi_annually',
  'half-yearly': 'semi_annually',
  annual: 'annually',
  yearly: 'annually',
  'one-time': 'one_time',
  onetime: 'one_time',
  once: 'one_time',
  single: 'one_time',
};

/** Canonical frequency for any known spelling, or `null` when unknown. */
export function parsePaymentFrequency(value: string | null | undefined): PaymentFrequency | null {
  if (!value) return null;
  const key = value.trim().toLowerCase();
  if ((PAYMENT_FREQUENCIES as readonly string[]).includes(key)) return key as PaymentFrequency;
  return LEGACY_ALIASES[key] ?? null;
}

/** Payments per year; 0 for one-time payments and for unknown frequencies. */
export function paymentsPerYear(value: string | null | undefined): number {
  switch (parsePaymentFrequency(value)) {
    case 'monthly':
      return 12;
    case 'quarterly':
      return 4;
    case 'semi_annually':
      return 2;
    case 'annually':
      return 1;
    case 'one_time':
    default:
      return 0;
  }
}

/** Yearly total of a recurring `amount` paid with `frequency`. */
export function annualizeAmount(amount: number, frequency: string | null | undefined): number {
  return amount * paymentsPerYear(frequency);
}
