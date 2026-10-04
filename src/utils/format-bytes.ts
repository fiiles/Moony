import { getFormatters } from '@/lib/format';

/** "1.2 MB" / "1,2 MB" with the UI locale's separators; bytes stay whole. */
export function formatBytes(bytes: number, locale: string): string {
  const units = ['B', 'kB', 'MB', 'GB'];
  let value = Math.max(0, bytes);
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 ? 0 : 1;
  return `${getFormatters(locale).number(value, { maximumFractionDigits: digits })}\u00a0${units[unit]}`;
}
