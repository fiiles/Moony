import { describe, expect, it } from 'vitest';
import { formatBytes } from './format-bytes';

describe('formatBytes', () => {
  it('picks the unit and keeps one decimal above bytes', () => {
    expect(formatBytes(512, 'en-US')).toBe('512 B');
    expect(formatBytes(1536, 'en-US')).toBe('1.5 kB');
    expect(formatBytes(5 * 1024 * 1024, 'en-US')).toBe('5 MB');
    expect(formatBytes(1.25 * 1024 ** 3, 'en-US')).toBe('1.3 GB');
  });

  it('uses the locale separators', () => {
    expect(formatBytes(1536, 'cs-CZ')).toBe('1,5 kB');
  });

  it('never goes negative', () => {
    expect(formatBytes(-10, 'en-US')).toBe('0 B');
  });
});
