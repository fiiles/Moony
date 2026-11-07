import { describe, expect, it } from 'vitest';
import { refetchedByPull } from './refetch-scope';

describe('refetchedByPull', () => {
  it('reads the local data again', () => {
    expect(refetchedByPull(['portfolio-metrics'])).toBe(true);
    expect(refetchedByPull(['bank-transactions', 'categorize-queue', 'all'])).toBe(true);
    expect(refetchedByPull([])).toBe(true);
  });

  it('leaves out the searches that reach the network', () => {
    expect(refetchedByPull(['stock-search', 'AAPL'])).toBe(false);
    expect(refetchedByPull(['crypto-search', 'btc'])).toBe(false);
  });
});
