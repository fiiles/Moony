/**
 * A list as the language writes it (audit 2026-11-15 B9): "USD, EUR a GBP", "Akcie, Krypto und
 * Dluhopisy", "株式、暗号資産" — never a comma joined in code, which Japanese and Chinese write as
 * "、". `Intl.ListFormat`, the narrow conjunction (no "and" in English).
 */
export function listText(locale: string, items: readonly string[]): string {
  return new Intl.ListFormat(locale, { type: 'conjunction', style: 'narrow' }).format(items);
}

export { sentences } from '@/utils/sentences';
