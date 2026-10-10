import { describe, expect, it } from 'vitest';
import {
  MOVEMENT_FLOOR_EUR,
  detailSections,
  flowsWindow,
  hasRecentRows,
  ledgerPresetForMonth,
  monthOf,
  monthStart,
  monthlyFlows,
  movementFloor,
  movementThreshold,
  newestMonthWithRows,
  oldestMonthWithRows,
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

describe('movement floor', () => {
  // 1 EUR = 25 CZK = 1.1 USD; no quote for KWD
  const fx = { reference: 'EUR', units: { CZK: 25, USD: 1.1 } };

  it('is 400 EUR in the account currency', () => {
    expect(MOVEMENT_FLOOR_EUR).toBe(400);
    expect(movementFloor(fx, 'EUR')).toBe(400);
    expect(movementFloor(fx, 'CZK')).toBe(10_000);
    expect(movementFloor(fx, 'USD')).toBeCloseTo(440);
  });

  it('is 0 without a conversion, so only the share of the balance applies', () => {
    expect(movementFloor(fx, 'KWD')).toBe(0);
    expect(movementFloor(undefined, 'CZK')).toBe(0);
    expect(movementThreshold(50_000, movementFloor(fx, 'KWD'))).toBe(5_000);
  });
});

describe('movements (README §9 threshold)', () => {
  it('uses the larger of the floor and 10 % of the balance', () => {
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

describe('native booked activity', () => {
  it('does not add pending or foreign-currency units', () => {
    const rows = [
      {
        id: 'a',
        type: 'credit' as const,
        amount: '100',
        bookingDate: 1,
        status: 'booked',
        currency: 'CZK',
      },
      {
        id: 'b',
        type: 'credit' as const,
        amount: '1000',
        bookingDate: 1,
        status: 'pending',
        currency: 'CZK',
      },
      {
        id: 'c',
        type: 'debit' as const,
        amount: '50',
        bookingDate: 1,
        status: 'booked',
        currency: 'EUR',
      },
    ];
    const summary = summarizeFlow(rows, 'CZK');
    expect(summary.income).toBe(100);
    expect(summary.expense).toBe(0);
    expect(summary.count).toBe(1);
  });
});

const at = (iso: string) =>
  Date.UTC(+iso.slice(0, 4), +iso.slice(5, 7) - 1, +iso.slice(8, 10)) / 1000;
const row = (
  id: string,
  type: 'credit' | 'debit',
  amount: number,
  iso: string,
  status: 'booked' | 'pending' = 'booked'
): FlowTransaction => ({ id, type, amount: String(amount), bookingDate: at(iso), status });

describe('month helpers', () => {
  it('indexes UTC months and gives their first day', () => {
    expect(monthOf(at('2026-10-10'))).toBe(2026 * 12 + 9);
    expect(monthStart(2026 * 12 + 9)).toBe(at('2026-10-01'));
    expect(monthStart(2026 * 12 + 0)).toBe(at('2026-01-01'));
  });
});

describe('monthlyFlows', () => {
  const rows = [
    row('a', 'credit', 54000, '2026-09-25'),
    row('b', 'debit', 1240, '2026-09-30'),
    row('c', 'debit', 23000, '2026-08-05'),
    row('p', 'debit', 999, '2026-09-29', 'pending'),
    row('old', 'debit', 5, '2025-01-15'),
  ];
  it('returns the 12 months ending with the window end, booked rows only', () => {
    const flows = monthlyFlows(rows, monthOf(at('2026-10-10')));
    expect(flows).toHaveLength(12);
    expect(flows[0].month).toBe(monthOf(at('2025-11-01')));
    expect(flows[11]).toEqual({
      month: monthOf(at('2026-10-01')),
      income: 0,
      expense: 0,
      count: 0,
    });
    expect(flows[10]).toEqual({
      month: monthOf(at('2026-09-01')),
      income: 54000,
      expense: 1240,
      count: 2,
    });
    expect(flows[9]).toEqual({
      month: monthOf(at('2026-08-01')),
      income: 0,
      expense: 23000,
      count: 1,
    });
  });
  it('shifts with the window end', () => {
    const flows = monthlyFlows(rows, monthOf(at('2025-01-31')));
    expect(flows[11]).toEqual({
      month: monthOf(at('2025-01-01')),
      income: 0,
      expense: 5,
      count: 1,
    });
  });
});

describe('flowsWindow', () => {
  const current = monthOf(at('2026-10-10'));
  const rows = [row('a', 'credit', 1, '2026-09-25'), row('old', 'debit', 1, '2025-03-15')];
  it('selects the newest month with rows inside the window and limits the arrows', () => {
    const w = flowsWindow(rows, current, current);
    expect(w.months).toHaveLength(12);
    expect(w.selected).toBe(monthOf(at('2026-09-01')));
    expect(w.canNext).toBe(false);
    expect(w.canPrev).toBe(true);
  });
  it('stops going back when the window reaches the oldest row', () => {
    const w = flowsWindow(rows, monthOf(at('2026-02-01')), current);
    expect(w.months[0]).toBe(monthOf(at('2025-03-01')));
    expect(w.canPrev).toBe(false);
    expect(w.canNext).toBe(true);
    expect(w.selected).toBe(monthOf(at('2025-03-01')));
  });
  it('honours an override inside the window and ignores one outside', () => {
    expect(flowsWindow(rows, current, current, monthOf(at('2026-06-01'))).selected).toBe(
      monthOf(at('2026-06-01'))
    );
    expect(flowsWindow(rows, current, current, monthOf(at('2024-06-01'))).selected).toBe(
      monthOf(at('2026-09-01'))
    );
  });
  it('has no selection without rows in the window', () => {
    expect(flowsWindow([], current, current).selected).toBeNull();
    expect(newestMonthWithRows([])).toBeNull();
    expect(oldestMonthWithRows(rows)).toBe(monthOf(at('2025-03-01')));
  });
});

describe('hasRecentRows / detailSections / ledgerPresetForMonth', () => {
  const current = monthOf(at('2026-10-10'));
  it('counts a booked row within the 12 months ending with the current one', () => {
    expect(hasRecentRows([row('a', 'credit', 1, '2025-11-01')], current)).toBe(true);
    expect(hasRecentRows([row('a', 'credit', 1, '2025-10-31')], current)).toBe(false);
    expect(hasRecentRows([row('a', 'credit', 1, '2026-10-01', 'pending')], current)).toBe(false);
  });
  it('shows sections by data', () => {
    expect(
      detailSections({ balance: 450000, hasAnyTransaction: false, hasRecentRows: false })
    ).toEqual({ balance: true, flows: false, ledger: false });
    expect(detailSections({ balance: 0, hasAnyTransaction: true, hasRecentRows: true })).toEqual({
      balance: false,
      flows: true,
      ledger: true,
    });
    expect(
      detailSections({ balance: 84320, hasAnyTransaction: true, hasRecentRows: true })
    ).toEqual({ balance: true, flows: true, ledger: true });
    expect(detailSections({ balance: 0, hasAnyTransaction: true, hasRecentRows: false })).toEqual({
      balance: false,
      flows: false,
      ledger: true,
    });
    expect(detailSections({ balance: 0, hasAnyTransaction: false, hasRecentRows: false })).toEqual({
      balance: false,
      flows: false,
      ledger: false,
    });
  });
  it('maps the newest month to a ledger preset', () => {
    expect(ledgerPresetForMonth(current, current)).toBe('thisMonth');
    expect(ledgerPresetForMonth(current - 1, current)).toBe('lastMonth');
    expect(ledgerPresetForMonth(current - 2, current)).toBe('custom');
  });
});
