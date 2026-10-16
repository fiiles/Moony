import { describe, expect, it } from 'vitest';
import { balanceLine } from './balance-line';
const DAY = 86_400;
const TODAY = Date.UTC(2026, 9, 16) / 1000;
const rec = (daysAgo: number, amount: string, id = String(daysAgo)) => ({
  id,
  amount,
  asOf: TODAY - daysAgo * DAY,
});

describe('balanceLine', () => {
  it('steps through the records and holds the newest value until today', () => {
    const { points, marks } = balanceLine([rec(40, '100'), rec(10, '150')], TODAY, undefined);
    expect(points).toEqual([
      { t: TODAY - 40 * DAY, value: 100 },
      { t: TODAY - 10 * DAY, value: 150 },
      { t: TODAY, value: 150 },
    ]);
    expect(marks.map((m) => m.id)).toEqual(['40', '10']);
  });
  it('enters the window with the balance held before it', () => {
    const { points, marks } = balanceLine(
      [rec(40, '100'), rec(10, '150')],
      TODAY,
      TODAY - 30 * DAY
    );
    expect(points[0]).toEqual({ t: TODAY - 30 * DAY, value: 100 });
    expect(marks).toHaveLength(1);
  });
  it('is empty without a record and ignores records after today', () => {
    expect(balanceLine([], TODAY, undefined).points).toEqual([]);
    expect(balanceLine([rec(-3, '1')], TODAY, undefined).points).toEqual([]);
  });
  it('does not duplicate today', () => {
    expect(balanceLine([rec(0, '7')], TODAY, undefined).points).toEqual([{ t: TODAY, value: 7 }]);
  });
});
