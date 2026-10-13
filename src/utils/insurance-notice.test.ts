import { describe, expect, it } from 'vitest';
import { MAX_NOTICE_DAYS, parseNoticeDays } from './insurance-notice';

describe('parseNoticeDays', () => {
  it('reads whole days from 0 to 365; an empty field means no reminder', () => {
    expect(parseNoticeDays('43')).toBe(43);
    expect(parseNoticeDays(' 60 ')).toBe(60);
    expect(parseNoticeDays('0')).toBe(0);
    expect(parseNoticeDays('')).toBe(0);
    expect(parseNoticeDays(String(MAX_NOTICE_DAYS))).toBe(365);
  });

  it('rejects anything else', () => {
    for (const text of ['366', '-1', '4.5', '4,5', 'abc', '1e2', '1000']) {
      expect(parseNoticeDays(text), text).toBeNull();
    }
  });
});
