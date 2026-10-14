import { describe, expect, it } from 'vitest';
import type { DatedFact } from '@shared/schema';
import { MAX_JUMPS, detectNetWorthJumps, type JumpClass, type JumpDay } from './net-worth-jumps';

const DAY = 86400;
/** 2026-01-01 UTC. */
const T0 = 1_767_225_600;
const at = (n: number) => T0 + n * DAY;

type Classes = Record<JumpClass, number>;
const ZERO: Classes = {
  savings: 0,
  investments: 0,
  crypto: 0,
  bonds: 0,
  realEstate: 0,
  loans: 0,
  otherAssets: 0,
};

/**
 * `length` consecutive days from `base`; `changes[n]` adds to the classes from day n on (a step),
 * so a class holds its value until the next change.
 */
function series(
  length: number,
  base: Partial<Classes>,
  changes: Record<number, Partial<Classes>> = {}
): JumpDay[] {
  const classes: Classes = { ...ZERO, ...base };
  const out: JumpDay[] = [];
  for (let n = 0; n < length; n++) {
    for (const [cls, delta] of Object.entries(changes[n] ?? {})) {
      classes[cls as JumpClass] += delta;
    }
    const sum = Object.values(classes).reduce((a, b) => a + b, 0);
    out.push({ t: at(n), netWorth: sum, classes: { ...classes } });
  }
  return out;
}

function fact(day: number, cls: JumpClass, kind: string, name: string, extra = {}): DatedFact {
  return {
    day: at(day),
    kind,
    class: cls,
    name,
    amount: null,
    currency: null,
    previousAmount: null,
    count: null,
    ...extra,
  };
}

/** About 10 M of net worth: 2 M cash, 5 M stocks, 3 M of property. */
const BASE: Partial<Classes> = {
  savings: 2_000_000,
  investments: 5_000_000,
  realEstate: 3_000_000,
};

describe('detectNetWorthJumps', () => {
  it('marks a 6 % step explained by a property purchase', () => {
    const days = series(10, BASE, { 5: { realEstate: 600_000 } });
    const jumps = detectNetWorthJumps(days, [fact(5, 'realEstate', 'buy', 'Chlumec')]);
    expect(jumps).toHaveLength(1);
    const [jump] = jumps;
    expect(jump.kind).toBe('jump');
    expect(jump.t).toBe(at(5));
    expect(jump.delta).toBe(600_000);
    expect(jump.parts).toHaveLength(1);
    expect(jump.parts[0].cls).toBe('realEstate');
    expect(jump.parts[0].facts.map((f) => f.name)).toEqual(['Chlumec']);
  });

  it('leaves out a 7 % stock-only move without trades: that is the market', () => {
    const days = series(10, BASE, { 5: { investments: 700_000 } });
    expect(detectNetWorthJumps(days, [])).toEqual([]);
  });

  it('marks an 11 % stock-only move as a market move', () => {
    const days = series(10, BASE, { 5: { investments: 1_200_000 } });
    const jumps = detectNetWorthJumps(days, []);
    expect(jumps).toHaveLength(1);
    expect(jumps[0].kind).toBe('market');
    expect(jumps[0].parts.map((p) => p.cls)).toEqual(['investments']);
    expect(jumps[0].parts[0].facts).toEqual([]);
  });

  it('marks a 6 % stock step on a day with a big buy as a plain jump', () => {
    const days = series(10, BASE, { 5: { investments: 600_000 } });
    const jumps = detectNetWorthJumps(days, [
      fact(5, 'investments', 'buy', 'AAPL, MSFT', { count: 2 }),
    ]);
    expect(jumps).toHaveLength(1);
    expect(jumps[0].kind).toBe('jump');
    expect(jumps[0].parts[0].facts[0].count).toBe(2);
  });

  it('marks a loan start alone: -2.1 M on a net worth of about 10 M', () => {
    const days = series(10, BASE, { 6: { loans: -2_100_000 } });
    const jumps = detectNetWorthJumps(days, [fact(6, 'loans', 'start', 'Chlumec - byt')]);
    expect(jumps).toHaveLength(1);
    expect(jumps[0].kind).toBe('jump');
    expect(jumps[0].delta).toBe(-2_100_000);
    expect(jumps[0].parts).toHaveLength(1);
    expect(jumps[0].parts[0].cls).toBe('loans');
    expect(jumps[0].parts[0].delta).toBe(-2_100_000);
    expect(jumps[0].parts[0].facts[0]).toMatchObject({ kind: 'start', name: 'Chlumec - byt' });
  });

  it('lists a property purchase and the loan drawn the same day, though they partly cancel', () => {
    const days = series(10, BASE, { 5: { realEstate: 3_120_000, loans: -1_970_000 } });
    const jumps = detectNetWorthJumps(days, [
      fact(5, 'loans', 'start', 'Chlumec - byt'),
      fact(5, 'realEstate', 'buy', 'Chlumec'),
    ]);
    expect(jumps).toHaveLength(1);
    expect(jumps[0].delta).toBe(1_150_000);
    expect(jumps[0].kind).toBe('jump');
    // Largest first; each class keeps the facts of its own
    expect(jumps[0].parts.map((p) => [p.cls, p.delta])).toEqual([
      ['realEstate', 3_120_000],
      ['loans', -1_970_000],
    ]);
    expect(jumps[0].parts[0].facts.map((f) => f.kind)).toEqual(['buy']);
    expect(jumps[0].parts[1].facts.map((f) => f.kind)).toEqual(['start']);
  });

  it('marks a manual loan balance that lowers the debt', () => {
    // 2.0 M owed became 1.4 M: the class contributes 0.6 M more
    const days = series(10, { ...BASE, loans: -2_000_000 }, { 4: { loans: 600_000 } });
    const jumps = detectNetWorthJumps(days, [
      fact(4, 'loans', 'balance', 'Hypotéka', {
        amount: '1400000',
        previousAmount: '2000000.00',
        currency: 'CZK',
      }),
    ]);
    expect(jumps).toHaveLength(1);
    expect(jumps[0].kind).toBe('jump');
    expect(jumps[0].parts[0].cls).toBe('loans');
    expect(jumps[0].parts[0].facts[0]).toMatchObject({
      kind: 'balance',
      amount: '1400000',
      previousAmount: '2000000.00',
    });
  });

  it('does not call a day a market move when a loan with a dated fact moved with the stocks', () => {
    // Stocks +1.3 M with no trade, a manual balance -1.0 M of debt: 43 % explained, but the loan
    // is something the user did
    const days = series(
      10,
      { ...BASE, loans: -3_000_000 },
      { 5: { investments: 1_300_000, loans: 1_000_000 } }
    );
    const withFact = detectNetWorthJumps(days, [fact(5, 'loans', 'balance', 'Hypotéka')]);
    expect(withFact).toHaveLength(1);
    expect(withFact[0].kind).toBe('jump');
    expect(withFact[0].parts.map((p) => p.cls)).toEqual(['investments', 'loans']);

    // Without the fact the same step is the market's
    const bare = detectNetWorthJumps(days, []);
    expect(bare).toHaveLength(1);
    expect(bare[0].kind).toBe('market');
  });

  it('marks a step no fact explains in a class other than stocks and crypto', () => {
    // A salary arriving: savings +0.6 M and no balance record that day
    const days = series(10, BASE, { 5: { savings: 600_000 } });
    const jumps = detectNetWorthJumps(days, []);
    expect(jumps).toHaveLength(1);
    expect(jumps[0].kind).toBe('jump');
    expect(jumps[0].parts[0]).toMatchObject({ cls: 'savings', delta: 600_000, facts: [] });
  });

  it('keeps a step a stock trade explains when the market moved too', () => {
    // Stocks moved 0.7 M on a day with a trade: whatever the market added, there was a trade
    const days = series(10, BASE, { 5: { investments: 700_000 } });
    const jumps = detectNetWorthJumps(days, [fact(5, 'investments', 'buy', 'AAPL')]);
    expect(jumps).toHaveLength(1);
    expect(jumps[0].kind).toBe('jump');
  });

  it('names only the classes that moved a tenth of the step, largest first', () => {
    const days = series(10, BASE, {
      5: { realEstate: 3_000_000, savings: 200_000, bonds: 50_000 },
    });
    const [jump] = detectNetWorthJumps(days, [fact(5, 'realEstate', 'buy', 'Byt')]);
    expect(jump.parts.map((p) => p.cls)).toEqual(['realEstate']);
  });

  it('ignores a step below 5 % of the largest net worth', () => {
    const days = series(10, BASE, { 5: { realEstate: 400_000 } });
    expect(detectNetWorthJumps(days, [fact(5, 'realEstate', 'buy', 'Byt')])).toEqual([]);
  });

  it('measures against the largest |net worth| of the period, not of the day', () => {
    // 10 M early on, then a fall to 4 M: the 0.55 M step at the bottom is over 5 % of the 10 M
    const days = series(10, BASE, { 3: { investments: -6_000_000 }, 8: { savings: 550_000 } });
    const jumps = detectNetWorthJumps(days, [fact(8, 'savings', 'balance', 'Účet')]);
    expect(jumps.map((j) => j.t)).toEqual([at(3), at(8)]);
  });

  it('never compares across a missing day', () => {
    const days = series(10, BASE, { 5: { realEstate: 3_000_000 } }).filter((_, n) => n !== 4);
    expect(detectNetWorthJumps(days, [fact(5, 'realEstate', 'buy', 'Byt')])).toEqual([]);
  });

  it('matches the facts of the day of a live point that is not at midnight', () => {
    const days = series(10, BASE, { 9: { realEstate: 700_000 } });
    days[9] = { ...days[9], t: at(9) + 51_000 };
    const jumps = detectNetWorthJumps(days, [fact(9, 'realEstate', 'buy', 'Byt')]);
    expect(jumps).toHaveLength(1);
    expect(jumps[0].t).toBe(at(9) + 51_000);
    expect(jumps[0].parts[0].facts).toHaveLength(1);
  });

  it('ignores the facts of other days', () => {
    const days = series(10, BASE, { 5: { realEstate: 600_000 } });
    const [jump] = detectNetWorthJumps(days, [fact(4, 'realEstate', 'buy', 'Včera')]);
    expect(jump.parts[0].facts).toEqual([]);
  });

  it('keeps the 12 largest steps, in time order', () => {
    // 15 steps up and down of 0.60 M, 0.61 M, ... 0.74 M around 10 M; the three smallest drop out
    const changes: Record<number, Partial<Classes>> = {};
    for (let n = 0; n < 15; n++) {
      changes[2 * n + 2] = { savings: (n % 2 === 0 ? 1 : -1) * (600_000 + n * 10_000) };
    }
    const jumps = detectNetWorthJumps(series(34, BASE, changes), []);
    expect(jumps).toHaveLength(MAX_JUMPS);
    expect(jumps.map((j) => j.t)).toEqual([...jumps.map((j) => j.t)].sort((a, b) => a - b));
    expect(Math.min(...jumps.map((j) => Math.abs(j.delta)))).toBe(630_000);
  });

  it('copes with a flat or empty period', () => {
    expect(detectNetWorthJumps([], [])).toEqual([]);
    expect(detectNetWorthJumps(series(5, {}), [])).toEqual([]);
    expect(detectNetWorthJumps(series(1, BASE), [])).toEqual([]);
  });
});
