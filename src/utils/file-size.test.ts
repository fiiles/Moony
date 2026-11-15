import { describe, expect, it } from 'vitest';
import { getFormatters } from '@/lib/format';
import { fileSizeText, formatFileSize } from './file-size';

const cs = getFormatters('cs-CZ', 'CZK');
const en = getFormatters('en-US', 'USD');

describe('formatFileSize', () => {
  it('is empty without a size', () => {
    expect(formatFileSize(null)).toBe('');
    expect(formatFileSize(0)).toBe('');
  });

  it('uses bytes, whole kB and one-decimal MB', () => {
    expect(formatFileSize(512)).toBe('512 B');
    expect(formatFileSize(2048)).toBe('2 kB');
    expect(formatFileSize(3 * 1024 * 1024)).toBe('3.0 MB');
  });
});

describe('fileSizeText', () => {
  it('writes bytes, kilobytes without decimals and megabytes with one', () => {
    expect(fileSizeText(512, cs)).toBe('512 B');
    expect(fileSizeText(830 * 1024, cs)).toBe('830 kB');
    expect(fileSizeText(Math.round(1.2 * 1024 * 1024), cs)).toBe('1,2 MB');
    expect(fileSizeText(Math.round(1.2 * 1024 * 1024), en)).toBe('1.2 MB');
    expect(fileSizeText(100 * 1024 * 1024, cs)).toBe('100 MB');
  });

  it('never writes 1024 kB', () => {
    expect(fileSizeText(1024 * 1024 - 1, cs)).toBe('1 MB');
  });
});
