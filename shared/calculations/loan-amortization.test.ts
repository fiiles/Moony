import { describe, expect, it } from 'vitest';
import fixture from './loan-amortization.fixture.json';
import {
  MAX_SCHEDULE_ROWS,
  amortizationSchedule,
  amortizationStep,
  dueDay,
  loanTermsFromWire,
  loanValuationMode,
  outstandingBalanceAt,
  paymentBelowInterest,
  paymentsDueThrough,
  elapsedTo,
  type LoanTerms,
} from './loan-amortization';

/** UTC-midnight unix seconds of an ISO date. */
function day(iso: string): number {
  const [y, m, d] = iso.split('-').map(Number);
  return Date.UTC(y, m - 1, d) / 1000;
}

const cents = (value: number) => Math.round(value * 100);

function terms(startIso: string, overrides: Partial<LoanTerms> = {}): LoanTerms {
  return {
    principal: 100_000,
    annualRatePct: 6,
    monthlyPayment: 2_000,
    startDay: day(startIso),
    ...overrides,
  };
}

describe('dueDay / paymentsDueThrough', () => {
  it('clamps the due day to the last day of short months', () => {
    const anchor = day('2024-01-31');
    expect(dueDay(anchor, 0)).toBe(anchor);
    expect(dueDay(anchor, 1)).toBe(day('2024-02-29'));
    expect(dueDay(anchor, 2)).toBe(day('2024-03-31'));
    expect(dueDay(anchor, 3)).toBe(day('2024-04-30'));
    expect(dueDay(anchor, 13)).toBe(day('2025-02-28'));
    expect(dueDay(day('2023-12-15'), 2)).toBe(day('2024-02-15'));
  });

  it('counts whole months, inclusive of the due day', () => {
    const anchor = day('2020-01-15');
    expect(paymentsDueThrough(anchor, day('2020-01-14'))).toBe(0);
    expect(paymentsDueThrough(anchor, day('2020-02-14'))).toBe(0);
    expect(paymentsDueThrough(anchor, day('2020-02-15'))).toBe(1);
    expect(paymentsDueThrough(anchor, day('2021-01-14'))).toBe(11);
    expect(paymentsDueThrough(anchor, day('2021-01-15'))).toBe(12);
    const monthEnd = day('2024-01-31');
    expect(paymentsDueThrough(monthEnd, day('2024-02-28'))).toBe(0);
    expect(paymentsDueThrough(monthEnd, day('2024-02-29'))).toBe(1);
    expect(paymentsDueThrough(monthEnd, day('2025-02-28'))).toBe(13);
  });

  it('treats a timestamp inside a day as that day', () => {
    expect(paymentsDueThrough(day('2020-01-15'), day('2020-02-15') + 12 * 3600)).toBe(1);
  });
});

describe('amortizationStep', () => {
  it('splits a payment into interest and principal', () => {
    const step = amortizationStep(100_000, 0.005, 2_000);
    expect(step.interest).toBeCloseTo(500, 9);
    expect(step.principalPart).toBeCloseTo(1_500, 9);
    expect(step.balanceAfter).toBeCloseTo(98_500, 9);
    expect(step.payment).toBe(2_000);
  });

  it('never overshoots and shrinks the final payment', () => {
    const step = amortizationStep(1_000, 0.01, 5_000);
    expect(step.balanceAfter).toBe(0);
    expect(step.principalPart).toBe(1_000);
    expect(step.payment).toBeCloseTo(1_010, 9);
  });

  it('grows the balance when the payment is below the interest', () => {
    const step = amortizationStep(100_000, 0.01, 500);
    expect(step.principalPart).toBeLessThan(0);
    expect(step.balanceAfter).toBeCloseTo(100_500, 9);
  });
});

describe('outstandingBalanceAt', () => {
  it('pins the 30-year annuity: 1 000 000 at 5 %, payment 5 368.22', () => {
    const t = terms('2020-01-15', {
      principal: 1_000_000,
      annualRatePct: 5,
      monthlyPayment: 5368.22,
    });
    expect(cents(outstandingBalanceAt(t, day('2021-01-15')))).toBe(98_524_630);
    expect(amortizationSchedule(t).rows).toHaveLength(360);
  });

  it('is the anchor amount before the anchor day', () => {
    const t = terms('2024-06-15', { anchorAmount: 70_000, anchorDay: day('2025-01-10') });
    expect(outstandingBalanceAt(t, day('2024-12-31'))).toBe(70_000);
    expect(outstandingBalanceAt(t, day('2025-01-10'))).toBe(70_000);
    expect(outstandingBalanceAt(terms('2024-06-15'), day('2020-01-01'))).toBe(100_000);
  });

  it('amortizes from a manual anchor instead of the principal and start day', () => {
    const t = terms('2020-01-01', { anchorAmount: 50_000, anchorDay: day('2025-03-20') });
    expect(cents(outstandingBalanceAt(t, day('2025-04-20')))).toBe(cents(48_250));
  });

  it('does not amortize without a monthly payment', () => {
    const t = terms('2020-01-01', { monthlyPayment: 0 });
    expect(outstandingBalanceAt(t, day('2040-01-01'))).toBe(100_000);
    expect(amortizationSchedule(t)).toEqual({ rows: [], end: 'noPayment' });
    expect(loanValuationMode(t)).toBe('static');
  });

  it('lets the balance grow when the payment is below the interest', () => {
    const t = terms('2020-01-01', { monthlyPayment: 400 });
    expect(paymentBelowInterest(t)).toBe(true);
    expect(outstandingBalanceAt(t, day('2021-01-01'))).toBeGreaterThan(100_000);
    expect(paymentBelowInterest(terms('2020-01-01', { monthlyPayment: 500 }))).toBe(true);
    expect(paymentBelowInterest(terms('2020-01-01', { monthlyPayment: 501 }))).toBe(false);
    expect(paymentBelowInterest(terms('2020-01-01', { monthlyPayment: 0 }))).toBe(false);
  });

  it('treats a rate of 0 as no interest', () => {
    const t = terms('2024-03-31', { principal: 120_000, annualRatePct: 0, monthlyPayment: 1_000 });
    expect(outstandingBalanceAt(t, day('2024-10-31'))).toBe(113_000);
  });

  it('reports valuation modes', () => {
    expect(loanValuationMode(terms('2020-01-01'))).toBe('automatic');
    expect(
      loanValuationMode(terms('2020-01-01', { anchorAmount: 1, anchorDay: day('2021-01-01') }))
    ).toBe('manual');
  });
});

describe('elapsedTo', () => {
  it('tracks interest paid and payments made through a payoff', () => {
    const t = terms('2024-01-31', { principal: 10_000, annualRatePct: 12, monthlyPayment: 3_000 });
    const two = elapsedTo(t, day('2024-03-31'));
    expect(two.paymentsMade).toBe(2);
    expect(cents(two.interestPaid)).toBe(cents(171));
    const done = elapsedTo(t, day('2030-01-01'));
    expect(done.paymentsMade).toBe(4);
    expect(done.balance).toBe(0);
  });
});

describe('amortizationSchedule', () => {
  it('chains rows and agrees with the valuation after every payment', () => {
    const t = terms('2022-08-31');
    const { rows, end } = amortizationSchedule(t);
    expect(end).toBe('paid');
    let previous = t.principal;
    for (const row of rows) {
      expect(previous + row.interest - row.payment - row.balanceAfter).toBeCloseTo(0, 6);
      expect(row.payment - row.interest - row.principalPart).toBeCloseTo(0, 6);
      expect(outstandingBalanceAt(t, row.dueDay)).toBeCloseTo(row.balanceAfter, 6);
      previous = row.balanceAfter;
    }
    expect(rows[rows.length - 1].balanceAfter).toBe(0);
  });

  it('stops at the end date without changing the valuation', () => {
    const t = terms('2024-01-15', { endDay: day('2024-06-15') });
    const { rows, end } = amortizationSchedule(t);
    expect(end).toBe('endDate');
    expect(rows).toHaveLength(5);
    expect(rows[4].dueDay).toBe(day('2024-06-15'));
    expect(cents(outstandingBalanceAt(t, day('2025-01-15')))).toBe(
      cents(outstandingBalanceAt(terms('2024-01-15'), day('2025-01-15')))
    );
  });

  it('is capped at MAX_SCHEDULE_ROWS for an interest-only loan', () => {
    const t = terms('2024-01-15', { monthlyPayment: 500 });
    const { rows, end } = amortizationSchedule(t);
    expect(rows).toHaveLength(MAX_SCHEDULE_ROWS);
    expect(end).toBe('capped');
  });

  it('is empty and paid for a zero-balance anchor', () => {
    const t = terms('2020-01-01', { anchorAmount: 0, anchorDay: day('2024-01-01') });
    expect(amortizationSchedule(t)).toEqual({ rows: [], end: 'paid' });
  });
});

describe('loanTermsFromWire', () => {
  const wire = {
    principal: '1000000',
    interestRate: '',
    monthlyPayment: '5000',
    startDate: day('2020-01-15') + 7 * 3600,
    endDate: null,
    balanceAnchorAmount: null,
    balanceAnchorDate: null,
  };

  it('floors days, reads an empty rate as 0 and has no anchor by default', () => {
    const t = loanTermsFromWire(wire);
    expect(t.annualRatePct).toBe(0);
    expect(t.startDay).toBe(day('2020-01-15'));
    expect(t.anchorAmount).toBeNull();
    expect(loanValuationMode(t)).toBe('automatic');
  });

  it('carries a manual anchor and the end date', () => {
    const t = loanTermsFromWire({
      ...wire,
      balanceAnchorAmount: '850000.50',
      balanceAnchorDate: day('2025-06-30'),
      endDate: day('2045-01-15'),
    });
    expect(t.anchorAmount).toBe(850000.5);
    expect(t.anchorDay).toBe(day('2025-06-30'));
    expect(t.endDay).toBe(day('2045-01-15'));
    expect(loanValuationMode(t)).toBe('manual');
  });
});

describe('Rust / TypeScript cross-check (shared fixture, to the cent)', () => {
  interface FixtureCase {
    name: string;
    terms: {
      principal: number;
      annualRatePct: number;
      monthlyPayment: number;
      startDay: string;
      anchorAmount: number | null;
      anchorDay: string | null;
      endDay: string | null;
    };
    balances: { on: string; balance: number }[];
    schedule?: {
      maxRows: number;
      end: string;
      rowCount: number;
      lastBalance: number | null;
      firstRows?: Record<string, number | string>[];
      lastRow?: Record<string, number | string>;
    };
  }
  const cases = fixture.cases as FixtureCase[];

  function fixtureTerms(c: FixtureCase): LoanTerms {
    return {
      principal: c.terms.principal,
      annualRatePct: c.terms.annualRatePct,
      monthlyPayment: c.terms.monthlyPayment,
      startDay: day(c.terms.startDay),
      anchorAmount: c.terms.anchorAmount,
      anchorDay: c.terms.anchorDay ? day(c.terms.anchorDay) : null,
      endDay: c.terms.endDay ? day(c.terms.endDay) : null,
    };
  }

  it('has the documented scenarios', () => {
    expect(cases.length).toBeGreaterThanOrEqual(8);
  });

  for (const c of cases) {
    it(`${c.name}: balances`, () => {
      const t = fixtureTerms(c);
      for (const b of c.balances) {
        expect(cents(outstandingBalanceAt(t, day(b.on))), `${c.name} on ${b.on}`).toBe(
          cents(b.balance)
        );
      }
    });

    if (c.schedule) {
      const expected = c.schedule;
      it(`${c.name}: schedule`, () => {
        const s = amortizationSchedule(fixtureTerms(c), expected.maxRows);
        expect(s.end).toBe(expected.end);
        expect(s.rows).toHaveLength(expected.rowCount);
        if (expected.lastBalance === null) {
          expect(s.rows).toHaveLength(0);
        } else {
          expect(cents(s.rows[s.rows.length - 1].balanceAfter)).toBe(cents(expected.lastBalance));
        }
        const check = (row: (typeof s.rows)[number], want: Record<string, number | string>) => {
          expect(row.dueDay).toBe(day(want.dueDay as string));
          expect(cents(row.payment)).toBe(cents(want.payment as number));
          expect(cents(row.interest)).toBe(cents(want.interest as number));
          expect(cents(row.principalPart)).toBe(cents(want.principalPart as number));
          expect(cents(row.balanceAfter)).toBe(cents(want.balanceAfter as number));
        };
        (expected.firstRows ?? []).forEach((want, i) => check(s.rows[i], want));
        if (expected.lastRow) check(s.rows[s.rows.length - 1], expected.lastRow);
      });
    }
  }
});
