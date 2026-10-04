import { describe, expect, it } from 'vitest';
import {
  movementThreshold,
  reconstructBalance,
  significantMovements,
  summarizeFlow,
  type FlowTransaction,
} from './bank-activity';

const DAY = 86400;
const D0 = 1_700_000_000 - (1_700_000_000 % DAY); // a UTC midnight

const tx = (
  id: string,
  type: 'credit' | 'debit',
  amount: number,
  day: number,
  categoryId: string | null = 'c1'
): FlowTransaction => ({
  id,
  type,
  amount: String(amount),
  bookingDate: D0 + day * DAY,
  categoryId,
});

describe('summarizeFlow', () => {
  it('sums income and expense, counts rows and the uncategorized ones', () => {
    const s = summarizeFlow([
      tx('a', 'credit', 1000, 0),
      tx('b', 'debit', 300, 1),
      tx('c', 'debit', 200, 2, null),
    ]);
    expect(s.income).toBe(1000);
    expect(s.expense).toBe(500);
    expect(s.net).toBe(500);
    expect(s.count).toBe(3);
    expect(s.incomeCount).toBe(1);
    expect(s.expenseCount).toBe(2);
    expect(s.uncategorized).toBe(1);
  });

  it('finds the category with the largest expense and its share', () => {
    const s = summarizeFlow([
      tx('a', 'debit', 100, 0, 'food'),
      tx('b', 'debit', 300, 1, 'housing'),
      tx('c', 'debit', 100, 2, 'food'),
      tx('d', 'credit', 5000, 3, 'salary'),
    ]);
    expect(s.topExpenseCategory).toEqual({ id: 'housing', amount: 300, share: 0.6 });
  });

  it('handles an empty list', () => {
    const s = summarizeFlow([]);
    expect(s.net).toBe(0);
    expect(s.topExpenseCategory).toBeNull();
  });
});

describe('reconstructBalance', () => {
  it('walks back from the current balance through the days', () => {
    const points = reconstructBalance(
      1000,
      [tx('a', 'credit', 500, 2), tx('b', 'debit', 200, 1)],
      D0,
      D0 + 2 * DAY
    );
    expect(points.map((p) => p.value)).toEqual([700, 500, 1000]);
    expect(points.map((p) => p.t)).toEqual([D0, D0 + DAY, D0 + 2 * DAY]);
  });

  it('undoes transactions after the end of the span first', () => {
    const points = reconstructBalance(1000, [tx('a', 'credit', 400, 5)], D0, D0 + 2 * DAY);
    expect(points.every((p) => p.value === 600)).toBe(true);
  });

  it('returns nothing for an inverted span', () => {
    expect(reconstructBalance(1, [], D0 + DAY, D0)).toEqual([]);
  });
});

describe('movements (README §9 threshold)', () => {
  it('uses the larger of 10 000 Kč and 10 % of the balance', () => {
    expect(movementThreshold(50_000, 10_000)).toBe(10_000);
    expect(movementThreshold(500_000, 10_000)).toBe(50_000);
    expect(movementThreshold(-500_000, 10_000)).toBe(50_000);
  });

  it('keeps only movements at or above the threshold, chronologically', () => {
    const events = significantMovements(
      [tx('big', 'debit', 15_000, 3), tx('small', 'debit', 900, 1), tx('pay', 'credit', 60_000, 2)],
      10_000
    );
    expect(events.map((e) => e.id)).toEqual(['pay', 'big']);
    expect(events[0].type).toBe('income');
    expect(events[1].type).toBe('expense');
  });
});
