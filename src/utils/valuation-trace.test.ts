import { describe, it, expect } from 'vitest';
import { dailyValuations, valuationTrace, type TraceValuation } from './valuation-trace';

const DAY = 86_400;
const day = (n: number) => n * DAY;
/** An estimate of `value` on day number `d`, entered at `createdAt`. */
const est = (id: string, d: number, value: number, createdAt = day(d)): TraceValuation => ({
  id,
  t: day(d),
  value,
  createdAt,
});
const pt = (d: number, value: number) => ({ t: day(d), value });

describe('dailyValuations', () => {
  it('orders by day, then by creation', () => {
    const rows = [est('late', 20, 3), est('b', 10, 2, 2000), est('a', 5, 1)];
    expect(dailyValuations(rows).map((r) => r.id)).toEqual(['a', 'b', 'late']);
  });

  it('keeps only the last row created on a day', () => {
    const rows = [est('second', 10, 120, 2000), est('first', 10, 100, 1000), est('next', 11, 130)];
    expect(dailyValuations(rows).map((r) => r.id)).toEqual(['second', 'next']);
  });

  it('does not touch its input and hands back the same objects', () => {
    const rows = [est('b', 20, 2), est('a', 10, 1)];
    const out = dailyValuations(rows);
    expect(rows.map((r) => r.id)).toEqual(['b', 'a']);
    expect(out[0]).toBe(rows[1]);
  });
});

describe('valuationTrace', () => {
  it('has nothing to trace without an estimate', () => {
    expect(valuationTrace({ valuations: [], now: day(100) })).toEqual([]);
    expect(
      valuationTrace({ valuations: [], purchase: { t: day(1), value: 90 }, now: day(100) })
    ).toEqual([]);
  });

  it('runs a single estimate from its day to now', () => {
    expect(valuationTrace({ valuations: [est('a', 10, 100)], now: day(30) })).toEqual([
      pt(10, 100),
      pt(30, 100),
    ]);
  });

  it('steps up the day before every change instead of sloping across the gap', () => {
    const trace = valuationTrace({
      valuations: [est('a', 10, 100), est('b', 20, 130)],
      now: day(40),
    });
    expect(trace).toEqual([pt(10, 100), pt(19, 100), pt(20, 130), pt(40, 130)]);
  });

  it('adds no step point when the next estimate comes the day after', () => {
    const trace = valuationTrace({
      valuations: [est('a', 10, 100), est('b', 11, 130)],
      now: day(40),
    });
    expect(trace).toEqual([pt(10, 100), pt(11, 130), pt(40, 130)]);
  });

  it('starts at the purchase when it precedes the first estimate', () => {
    const trace = valuationTrace({
      valuations: [est('a', 10, 100)],
      purchase: { t: day(2), value: 90 },
      now: day(30),
    });
    expect(trace).toEqual([pt(2, 90), pt(9, 90), pt(10, 100), pt(30, 100)]);
  });

  it('needs no step between a purchase the day before the first estimate and that estimate', () => {
    const trace = valuationTrace({
      valuations: [est('a', 10, 100)],
      purchase: { t: day(9), value: 90 },
      now: day(30),
    });
    expect(trace).toEqual([pt(9, 90), pt(10, 100), pt(30, 100)]);
  });

  it('leaves the purchase out when it is not before the first estimate', () => {
    for (const purchaseDay of [10, 12]) {
      const trace = valuationTrace({
        valuations: [est('a', 10, 100), est('b', 20, 130)],
        purchase: { t: day(purchaseDay), value: 90 },
        now: day(40),
      });
      expect(trace).toEqual([pt(10, 100), pt(19, 100), pt(20, 130), pt(40, 130)]);
    }
  });

  it('collapses estimates of one day to the last one created, whatever the input order', () => {
    const rows = [est('a', 10, 100, 1000), est('b', 10, 120, 2000), est('c', 20, 130)];
    const expected = [pt(10, 120), pt(19, 120), pt(20, 130), pt(40, 130)];
    expect(valuationTrace({ valuations: rows, now: day(40) })).toEqual(expected);
    expect(valuationTrace({ valuations: [...rows].reverse(), now: day(40) })).toEqual(expected);
  });

  it('sorts estimates entered out of order', () => {
    const trace = valuationTrace({
      valuations: [est('b', 20, 130), est('a', 10, 100)],
      now: day(40),
    });
    expect(trace).toEqual([pt(10, 100), pt(19, 100), pt(20, 130), pt(40, 130)]);
  });

  it('adds the closing point only when now is after the last estimate', () => {
    const rows = [est('a', 10, 100), est('b', 30, 130)];
    expect(valuationTrace({ valuations: rows, now: day(30) }).at(-1)).toEqual(pt(30, 130));
    expect(valuationTrace({ valuations: rows, now: day(25) }).at(-1)).toEqual(pt(30, 130));
    expect(valuationTrace({ valuations: rows, now: day(30) + 3_600 }).at(-1)).toEqual({
      t: day(30) + 3_600,
      value: 130,
    });
  });

  describe('with a start', () => {
    const rows = [est('a', 10, 100), est('b', 20, 130)];
    const full = [pt(10, 100), pt(19, 100), pt(20, 130), pt(40, 130)];

    it('replaces everything before it with one anchor carrying the value that applied', () => {
      expect(valuationTrace({ valuations: rows, now: day(40), start: day(15) })).toEqual([
        pt(15, 100),
        pt(19, 100),
        pt(20, 130),
        pt(40, 130),
      ]);
    });

    it('changes nothing when it precedes the whole trace', () => {
      expect(valuationTrace({ valuations: rows, now: day(40), start: day(1) })).toEqual(full);
    });

    it('does not repeat a point it lands on', () => {
      expect(valuationTrace({ valuations: rows, now: day(40), start: day(19) })).toEqual(
        full.slice(1)
      );
      expect(valuationTrace({ valuations: rows, now: day(40), start: day(20) })).toEqual(
        full.slice(2)
      );
    });

    it('carries the last value when it follows the last estimate', () => {
      expect(valuationTrace({ valuations: rows, now: day(40), start: day(35) })).toEqual([
        pt(35, 130),
        pt(40, 130),
      ]);
    });

    it('anchors on the purchase value when the window opens between purchase and first estimate', () => {
      const trace = valuationTrace({
        valuations: [est('a', 10, 100)],
        purchase: { t: day(2), value: 90 },
        now: day(30),
        start: day(5),
      });
      expect(trace).toEqual([pt(5, 90), pt(9, 90), pt(10, 100), pt(30, 100)]);
    });
  });

  it('always returns strictly increasing days, whatever it is given', () => {
    // A small deterministic generator, so a failure reproduces.
    let seed = 12_345;
    const next = (max: number) => {
      seed = (seed * 1_103_515_245 + 12_345) % 2_147_483_648;
      return seed % max;
    };
    for (let round = 0; round < 300; round++) {
      const rows = Array.from({ length: next(8) }, (_, i) =>
        est(`r${i}`, next(40), 50 + next(100), next(5_000))
      );
      const purchase = next(3) === 0 ? null : { t: day(next(45)), value: 40 + next(50) };
      const trace = valuationTrace({
        valuations: rows,
        purchase,
        now: day(next(60)) + next(2) * 3_600,
        start: next(2) === 0 ? undefined : day(next(50)),
      });
      for (let i = 1; i < trace.length; i++) {
        expect(trace[i].t).toBeGreaterThan(trace[i - 1].t);
      }
    }
  });
});
