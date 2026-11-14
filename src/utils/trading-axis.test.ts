import { describe, expect, it } from 'vitest';
import { sessionStarts, tradingAxis, withLivePrice } from './trading-axis';

const H = 3600;
const M30 = 30 * 60;

/** Two US sessions (15:30–22:00 CET as 30-minute bars), a night between them. */
function twoSessions(): number[] {
  const day1 = Array.from({ length: 13 }, (_, i) => 1000 * H + i * M30);
  const day2 = day1.map((t) => t + 24 * H);
  return [...day1, ...day2];
}

describe('tradingAxis', () => {
  it('leaves an even series as it is', () => {
    const times = [0, 300, 600, 900];
    const axis = tradingAxis(times);
    expect(times.map(axis.toAxis)).toEqual(times);
  });

  it('shrinks the night between two sessions to one bar', () => {
    const times = twoSessions();
    const axis = tradingAxis(times);
    const xs = times.map(axis.toAxis);
    // Day 2 opens one bar after day 1 closes.
    expect(xs[13] - xs[12]).toBe(M30);
    // Bars inside a session keep their spacing.
    expect(xs[1] - xs[0]).toBe(M30);
    expect(xs[25] - xs[24]).toBe(M30);
    expect(xs[25] - xs[0]).toBe(25 * M30);
  });

  it('maps back to the real time, inside a shrunk gap too', () => {
    const times = twoSessions();
    const axis = tradingAxis(times);
    for (const t of [times[0], times[5], times[12] + 3 * H, times[13], times[25] + 900]) {
      expect(axis.fromAxis(axis.toAxis(t))).toBeCloseTo(t, 6);
    }
  });

  it('places a time inside the night proportionally within the shrunk gap', () => {
    const times = twoSessions();
    const axis = tradingAxis(times);
    const mid = (times[12] + times[13]) / 2;
    expect(axis.toAxis(mid)).toBeCloseTo(axis.toAxis(times[12]) + M30 / 2, 6);
  });

  it('extends linearly before the first and after the last point', () => {
    const axis = tradingAxis([100, 200, 300]);
    expect(axis.toAxis(50)).toBe(50);
    expect(axis.toAxis(400)).toBe(400);
    expect(axis.fromAxis(400)).toBe(400);
  });

  it('copes with one point or none', () => {
    expect(tradingAxis([]).toAxis(5)).toBe(5);
    expect(tradingAxis([42]).toAxis(42)).toBe(42);
  });
});

describe('sessionStarts', () => {
  it('names the first point of every session', () => {
    expect(sessionStarts(twoSessions())).toEqual([0, 13]);
  });

  it('is the first point alone for one session', () => {
    expect(sessionStarts([0, 300, 600])).toEqual([0]);
    expect(sessionStarts([])).toEqual([]);
  });
});

describe('withLivePrice', () => {
  const session = [0, 300, 600, 900].map((t) => ({ t, value: 10 }));

  it('appends the live price fetched during the session', () => {
    const pts = withLivePrice(session, 11, 1000);
    expect(pts).toHaveLength(5);
    expect(pts[4]).toEqual({ t: 1000, value: 11 });
  });

  it('ends on the live price without a flat line when fetched after the close', () => {
    const pts = withLivePrice(session, 11, 900 + 11 * H);
    expect(pts).toHaveLength(4);
    expect(pts[3]).toEqual({ t: 900, value: 11 });
  });

  it('updates the last point when it is newer than the fetch', () => {
    const pts = withLivePrice(session, 12, 800);
    expect(pts).toHaveLength(4);
    expect(pts[3].value).toBe(12);
  });

  it('appends after a single point (no spacing known)', () => {
    expect(withLivePrice([{ t: 0, value: 1 }], 2, 5000)).toHaveLength(2);
  });

  it('leaves the series alone without a usable price', () => {
    expect(withLivePrice(session, NaN, 1000)).toEqual(session);
    expect(withLivePrice([], 5, 1000)).toEqual([]);
  });

  it('does not touch the input', () => {
    const input = session.map((p) => ({ ...p }));
    withLivePrice(input, 99, 800);
    expect(input[3].value).toBe(10);
  });
});
