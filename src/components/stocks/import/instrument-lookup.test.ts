import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  StockImportInstrument,
  StockInstrumentQuery,
  StockInstrumentResolution,
} from '@shared/schema';
import { createInstrumentLookup, type LookupState } from './instrument-lookup';

const instrument = (key: string): StockImportInstrument => ({
  key,
  symbol: key.replace('symbol:', ''),
  isin: null,
  name: null,
  currency: 'USD',
  tradeCount: 1,
  ticker: key.replace('symbol:', ''),
  status: 'new',
  positionCurrency: null,
});

const unansweredFor = (key: string): StockInstrumentResolution => ({
  key,
  candidates: [],
  best: null,
  lookupFailed: true,
});

const found = (key: string): StockInstrumentResolution => ({
  key,
  candidates: [{ symbol: key.replace('symbol:', ''), name: key, exchange: 'NMS', currency: 'USD' }],
  best: null,
  lookupFailed: false,
});

/** A resolver whose answers the test releases by hand. */
function controlledResolver() {
  const calls: {
    queries: StockInstrumentQuery[];
    settle: (answers: StockInstrumentResolution[]) => void;
    fail: (e: unknown) => void;
  }[] = [];
  const resolve = (queries: StockInstrumentQuery[]) =>
    new Promise<StockInstrumentResolution[]>((resolveCall, rejectCall) => {
      calls.push({ queries, settle: resolveCall, fail: rejectCall });
    });
  return { resolve, calls };
}

/** Lets the promise chain of the lookup run until it waits on the resolver again. */
const flush = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};

describe('createInstrumentLookup', () => {
  let states: LookupState[];
  let resolved: { asked: string[]; answers: string[] }[];

  beforeEach(() => {
    states = [];
    resolved = [];
    vi.spyOn(console, 'error').mockImplementation(() => {});
  });
  afterEach(() => vi.restoreAllMocks());

  const make = (resolve: ReturnType<typeof controlledResolver>['resolve'], chunkSize = 2) =>
    createInstrumentLookup({
      resolve,
      chunkSize,
      onChange: (state) => states.push(state),
      onResolved: (asked, answers) =>
        resolved.push({ asked: asked.map((i) => i.key), answers: answers.map((a) => a.key) }),
    });
  const last = () => states[states.length - 1];

  it('asks in chunks, one call after the other, and reports progress', async () => {
    const { resolve, calls } = controlledResolver();
    const lookup = make(resolve);
    lookup.enqueue(['symbol:A', 'symbol:B', 'symbol:C'].map(instrument));

    expect(last().progress).toEqual({ done: 0, total: 3 });
    await flush();
    expect(calls).toHaveLength(1);
    expect(calls[0].queries.map((q) => q.key)).toEqual(['symbol:A', 'symbol:B']);

    calls[0].settle([found('symbol:A'), found('symbol:B')]);
    await flush();
    expect(last().progress).toEqual({ done: 2, total: 3 });
    expect(calls).toHaveLength(2);
    expect(calls[1].queries.map((q) => q.key)).toEqual(['symbol:C']);

    calls[1].settle([found('symbol:C')]);
    await flush();
    expect(last().progress).toEqual({ done: 3, total: 3 });
    expect(Object.keys(last().resolutions).sort()).toEqual(['symbol:A', 'symbol:B', 'symbol:C']);
    expect(resolved).toEqual([
      { asked: ['symbol:A', 'symbol:B'], answers: ['symbol:A', 'symbol:B'] },
      { asked: ['symbol:C'], answers: ['symbol:C'] },
    ]);
  });

  it('asks about an instrument once', async () => {
    const { resolve, calls } = controlledResolver();
    const lookup = make(resolve, 5);
    lookup.enqueue([instrument('symbol:A')]);
    lookup.enqueue([instrument('symbol:A'), instrument('symbol:B')]);
    await flush();
    expect(calls).toHaveLength(1);
    calls[0].settle([found('symbol:A')]);
    await flush();
    expect(calls).toHaveLength(2);
    expect(calls[1].queries.map((q) => q.key)).toEqual(['symbol:B']);
    expect(lookup.hasAsked('symbol:A')).toBe(true);
    expect(lookup.hasAsked('symbol:Z')).toBe(false);
  });

  it('never runs two batches at the same time', async () => {
    const { resolve, calls } = controlledResolver();
    const lookup = make(resolve, 5);
    lookup.enqueue([instrument('symbol:A')]);
    lookup.enqueue([instrument('symbol:B')]);
    await flush();
    expect(calls).toHaveLength(1);
    calls[0].settle([found('symbol:A')]);
    await flush();
    expect(calls).toHaveLength(2);
  });

  it('marks a call that fails as unverified and goes on', async () => {
    const { resolve, calls } = controlledResolver();
    const lookup = make(resolve, 1);
    lookup.enqueue([instrument('symbol:A'), instrument('symbol:B')]);
    await flush();
    calls[0].fail(new Error('offline'));
    await flush();
    expect(last().resolutions['symbol:A']).toMatchObject({ lookupFailed: true, candidates: [] });
    calls[1].settle([found('symbol:B')]);
    await flush();
    expect(last().resolutions['symbol:B'].lookupFailed).toBe(false);
    expect(last().progress).toEqual({ done: 2, total: 2 });
  });

  it('marks an instrument the answer leaves out as unverified, and drops answers nobody asked for', async () => {
    const { resolve, calls } = controlledResolver();
    const lookup = make(resolve, 5);
    lookup.enqueue([instrument('symbol:A'), instrument('symbol:B')]);
    await flush();
    calls[0].settle([found('symbol:A'), found('symbol:STRANGER')]);
    await flush();
    expect(last().resolutions['symbol:B'].lookupFailed).toBe(true);
    expect(last().resolutions['symbol:A'].lookupFailed).toBe(false);
    expect(last().resolutions['symbol:STRANGER']).toBeUndefined();
    expect(last().progress).toEqual({ done: 2, total: 2 });
  });

  it('stops waiting on skip: what has no answer is unverified and later answers are ignored', async () => {
    const { resolve, calls } = controlledResolver();
    const lookup = make(resolve, 1);
    lookup.enqueue([instrument('symbol:A'), instrument('symbol:B')]);
    await flush();
    lookup.skip();
    expect(last().progress).toEqual({ done: 2, total: 2 });
    expect(last().resolutions['symbol:A'].lookupFailed).toBe(true);
    expect(last().resolutions['symbol:B'].lookupFailed).toBe(true);

    calls[0].settle([found('symbol:A')]);
    await flush();
    expect(last().resolutions['symbol:A'].lookupFailed).toBe(true);
    expect(calls).toHaveLength(1);
    expect(resolved).toEqual([]);
  });

  it('looks later instruments up normally after a skip', async () => {
    const { resolve, calls } = controlledResolver();
    const lookup = make(resolve, 5);
    lookup.enqueue([instrument('symbol:A')]);
    await flush();
    lookup.skip();
    lookup.enqueue([instrument('symbol:B')]);
    await flush();
    calls[1].settle([found('symbol:B')]);
    await flush();
    expect(last().resolutions['symbol:B'].lookupFailed).toBe(false);
    expect(last().progress).toEqual({ done: 2, total: 2 });
  });

  it('asks again about the instruments Yahoo Finance did not answer for', async () => {
    const { resolve, calls } = controlledResolver();
    const lookup = make(resolve, 5);
    const all = [instrument('symbol:A'), instrument('symbol:B'), instrument('symbol:C')];
    lookup.enqueue(all);
    await flush();
    calls[0].settle([found('symbol:A'), unansweredFor('symbol:B'), unansweredFor('symbol:C')]);
    await flush();
    expect(last().resolutions['symbol:B'].lookupFailed).toBe(true);

    lookup.retryFailed(all);
    expect(last().progress).toEqual({ done: 1, total: 3 });
    expect(last().resolutions['symbol:B']).toBeUndefined();
    await flush();
    expect(calls).toHaveLength(2);
    expect(calls[1].queries.map((q) => q.key)).toEqual(['symbol:B', 'symbol:C']);
    calls[1].settle([found('symbol:B'), found('symbol:C')]);
    await flush();
    expect(last().resolutions['symbol:B'].lookupFailed).toBe(false);
    expect(last().progress).toEqual({ done: 3, total: 3 });
  });

  it('has nothing to retry when every answer came', async () => {
    const { resolve, calls } = controlledResolver();
    const lookup = make(resolve, 5);
    lookup.enqueue([instrument('symbol:A')]);
    await flush();
    calls[0].settle([found('symbol:A')]);
    await flush();
    lookup.retryFailed([instrument('symbol:A')]);
    await flush();
    expect(calls).toHaveLength(1);
  });

  it('forgets everything on reset, answers still in flight included', async () => {
    const { resolve, calls } = controlledResolver();
    const lookup = make(resolve, 5);
    lookup.enqueue([instrument('symbol:A')]);
    await flush();
    lookup.reset();
    expect(last()).toEqual({ resolutions: {}, progress: { done: 0, total: 0 } });
    calls[0].settle([found('symbol:A')]);
    await flush();
    expect(last().resolutions).toEqual({});
    expect(lookup.hasAsked('symbol:A')).toBe(false);

    lookup.enqueue([instrument('symbol:A')]);
    await flush();
    expect(calls).toHaveLength(2);
  });

  it('keeps a listing the user picked without asking the backend', () => {
    const { resolve, calls } = controlledResolver();
    const lookup = make(resolve);
    lookup.remember(found('symbol:A'));
    expect(calls).toHaveLength(0);
    expect(last().resolutions['symbol:A'].lookupFailed).toBe(false);
    expect(last().progress).toEqual({ done: 1, total: 1 });
  });

  it('confirms one typed symbol and treats a failure as unverified', async () => {
    const { resolve, calls } = controlledResolver();
    const lookup = make(resolve);
    const query: StockInstrumentQuery = {
      key: 'symbol:A',
      symbol: 'AAPL',
      isin: null,
      name: null,
      currency: 'USD',
    };

    const first = lookup.verify(query);
    await flush();
    calls[0].settle([found('symbol:A')]);
    await first;
    expect(last().resolutions['symbol:A'].lookupFailed).toBe(false);

    const second = lookup.verify(query);
    await flush();
    calls[1].fail(new Error('offline'));
    await second;
    expect(last().resolutions['symbol:A'].lookupFailed).toBe(true);
  });

  it('ignores the answer to a typed symbol after a reset', async () => {
    const { resolve, calls } = controlledResolver();
    const lookup = make(resolve);
    const pending = lookup.verify({
      key: 'symbol:A',
      symbol: 'AAPL',
      isin: null,
      name: null,
      currency: null,
    });
    await flush();
    lookup.reset();
    calls[0].settle([found('symbol:A')]);
    await pending;
    expect(last().resolutions).toEqual({});
  });
});
