import { describe, expect, it } from 'vitest';
import { formatFileSize } from './file-size';

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
