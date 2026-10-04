import { describe, expect, it } from 'vitest';
import { budgetMultiplier, budgetState, paceVerdict, periodPace } from './budget-pace';

const OCT_START = Date.UTC(2026, 9, 1) / 1000;
const OCT_END = Date.UTC(2026, 10, 1) / 1000 - 1;

describe('periodPace', () => {
  it('matches the prototype on 2 October: 6 % behind us, 29 days ahead', () => {
    const pace = periodPace(OCT_START, OCT_END, new Date(Date.UTC(2026, 9, 2, 14, 0)));
    expect(pace.days).toBe(31);
    expect(pace.elapsedDays).toBe(2);
    expect(pace.remainingDays).toBe(29);
    expect(Math.round(pace.elapsed * 100)).toBe(6);
    expect(pace.isCurrent).toBe(true);
    expect(pace.isPast).toBe(false);
  });

  it('counts the last day as fully elapsed with nothing ahead', () => {
    const pace = periodPace(OCT_START, OCT_END, new Date(Date.UTC(2026, 9, 31, 23, 30)));
    expect(pace.elapsedDays).toBe(31);
    expect(pace.remainingDays).toBe(0);
    expect(pace.elapsed).toBe(1);
    expect(pace.isCurrent).toBe(true);
  });

  it('treats a past month as closed and a future month as untouched', () => {
    const past = periodPace(OCT_START, OCT_END, new Date(Date.UTC(2026, 11, 5)));
    expect(past).toMatchObject({ elapsed: 1, remainingDays: 0, isPast: true, isCurrent: false });
    const future = periodPace(OCT_START, OCT_END, new Date(Date.UTC(2026, 8, 20)));
    expect(future).toMatchObject({
      elapsed: 0,
      remainingDays: 31,
      isPast: false,
      isCurrent: false,
    });
  });

  it('works for a quarter', () => {
    const start = Date.UTC(2026, 9, 1) / 1000;
    const end = Date.UTC(2027, 0, 1) / 1000 - 1;
    const pace = periodPace(start, end, new Date(Date.UTC(2026, 10, 15)));
    expect(pace.days).toBe(92);
    expect(pace.elapsedDays).toBe(46);
    expect(pace.remainingDays).toBe(46);
  });
});

describe('budgetState', () => {
  it('uses the prototype thresholds', () => {
    expect(budgetState(4820, 4000)).toBe('over');
    expect(budgetState(4000, 4000)).toBe('near');
    expect(budgetState(3400, 4000)).toBe('near');
    expect(budgetState(3399, 4000)).toBe('ok');
    expect(budgetState(0, 4000)).toBe('ok');
    expect(budgetState(500, 0)).toBe('ok');
  });
});

describe('budgetMultiplier', () => {
  it('scales monthly limits', () => {
    expect(budgetMultiplier('monthly')).toBe(1);
    expect(budgetMultiplier('quarterly')).toBe(3);
    expect(budgetMultiplier('yearly')).toBe(12);
  });
});

describe('paceVerdict', () => {
  it('flags spending that runs ahead of or behind the calendar by more than 10 points', () => {
    expect(paceVerdict(0.66, 0.06)).toEqual({ verdict: 'ahead', points: 60 });
    expect(paceVerdict(0.2, 0.5)).toEqual({ verdict: 'behind', points: 30 });
    expect(paceVerdict(0.52, 0.5)).toEqual({ verdict: 'onPace', points: 2 });
    expect(paceVerdict(0.4, 0.5)).toEqual({ verdict: 'onPace', points: 10 });
  });
});
