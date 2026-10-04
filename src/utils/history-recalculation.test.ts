import { describe, expect, it } from 'vitest';
import { applyRecalculationEvent } from './history-recalculation';

describe('applyRecalculationEvent', () => {
  it('adds a ticker when its rebuild starts', () => {
    expect(applyRecalculationEvent([], { ticker: 'AAPL', status: 'running' })).toEqual(['AAPL']);
    expect(applyRecalculationEvent(['AAPL'], { ticker: 'MSFT', status: 'running' })).toEqual([
      'AAPL',
      'MSFT',
    ]);
  });

  it('removes it when the rebuild is done or failed, leaving the others', () => {
    const running = ['AAPL', 'MSFT'];
    expect(applyRecalculationEvent(running, { ticker: 'AAPL', status: 'done' })).toEqual(['MSFT']);
    expect(applyRecalculationEvent(running, { ticker: 'MSFT', status: 'failed' })).toEqual([
      'AAPL',
    ]);
  });

  it('returns the same array for events that change nothing', () => {
    const running = ['AAPL'];
    expect(applyRecalculationEvent(running, { ticker: 'AAPL', status: 'running' })).toBe(running);
    expect(applyRecalculationEvent(running, { ticker: 'MSFT', status: 'done' })).toBe(running);
  });
});
