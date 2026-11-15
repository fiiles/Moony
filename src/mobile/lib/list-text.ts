/**
 * A list as the language writes it (audit 2026-11-15 B9): "USD, EUR a GBP", "Akcie, Krypto und
 * Dluhopisy", "株式、暗号資産" — never a comma joined in code, which Japanese and Chinese write as
 * "、". `Intl.ListFormat`, the narrow conjunction (no "and" in English).
 */
export function listText(locale: string, items: readonly string[]): string {
  return new Intl.ListFormat(locale, { type: 'conjunction', style: 'narrow' }).format(items);
}

/**
 * Whole sentences run on as the language writes them: after a space, or straight after the
 * full stop in Japanese and Chinese, whose "。" carries the space itself (B9).
 */
export function sentences(locale: string, parts: readonly string[]): string {
  const tight = /^(ja|zh)\b/i.test(locale);
  return parts.filter(Boolean).join(tight ? '' : ' ');
}
