import type { Formatters } from '@/lib/format';

/** A file size for a document row: bytes, whole kB or MB with one decimal; empty when unknown. */
export const formatFileSize = (bytes: number | null) => {
  if (!bytes) return '';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} kB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
};

const KILOBYTE = 1024;
const MEGABYTE = 1024 * 1024;

/** "512 B", "830 kB", "1,2 MB": the desktop's units, the number in the language's own form. */
export function fileSizeText(bytes: number, fmt: Pick<Formatters, 'number'>): string {
  if (bytes < KILOBYTE) return `${fmt.number(bytes)} B`;
  const kilobytes = Math.round(bytes / KILOBYTE);
  // 1 048 575 bytes round to 1024 kB: that is a megabyte
  if (kilobytes < KILOBYTE) return `${fmt.number(kilobytes)} kB`;
  return `${fmt.number(bytes / MEGABYTE, { maximumFractionDigits: 1 })} MB`;
}
