/**
 * Whole sentences run on as the language writes them: after a space, or straight after the
 * full stop in Japanese and Chinese, whose "。" carries the space itself (audit 2026-11-15 B9).
 * Empty parts are left out.
 */
export function sentences(
  locale: string,
  parts: readonly (string | null | undefined | false)[]
): string {
  const tight = /^(ja|zh)\b/i.test(locale);
  return parts.filter(Boolean).join(tight ? '' : ' ');
}
