import { describe, expect, it } from 'vitest';
import type { Milestone } from '@shared/schema';
import { agendaRowParts } from './agenda-row';

const base = {
  key: 'k',
  title: 'T',
  sourceId: 's',
  dueDay: 1_800_000_000,
  amount: null,
  currency: null,
  referenceAmount: null,
  direction: null,
  count: null,
  sinceDay: null,
  actionDay: null,
  tone: 'action',
  stage: 'upcoming',
} as unknown as Milestone;

const m = (over: Partial<Milestone>) => ({ ...base, ...over }) as Milestone;

describe('agendaRowParts', () => {
  it('moves the amount of a payment, a coupon and a maturity to the value', () => {
    expect(agendaRowParts(m({ kind: 'bond_maturity', amount: '100000', currency: 'CZK' }))).toEqual(
      { what: 'bond_maturity', amount: { value: 100000, currency: 'CZK' } }
    );
    expect(agendaRowParts(m({ kind: 'bond_coupon', amount: '2500.5', currency: 'EUR' }))).toEqual({
      what: 'bond_coupon',
      amount: { value: 2500.5, currency: 'EUR' },
    });
    expect(
      agendaRowParts(m({ kind: 'insurance_payment', amount: '1290', currency: 'CZK' }))
    ).toEqual({ what: 'insurance_payment', amount: { value: 1290, currency: 'CZK' } });
  });

  it('leaves the value empty when the amount is missing or not a number', () => {
    expect(agendaRowParts(m({ kind: 'bond_coupon', amount: null })).amount).toBeNull();
    expect(
      agendaRowParts(m({ kind: 'bond_coupon', amount: 'x', currency: 'CZK' })).amount
    ).toBeNull();
  });

  it('says a notice deadline in one fact, its passed form apart', () => {
    expect(agendaRowParts(m({ kind: 'insurance_anniversary' })).what).toBe('insurance_anniversary');
    expect(agendaRowParts(m({ kind: 'insurance_anniversary', tone: 'info' })).what).toBe(
      'insurance_anniversary_passed'
    );
  });

  it('puts the crossed price in the value and the target in the sub', () => {
    expect(
      agendaRowParts(
        m({
          kind: 'watch_target',
          direction: 'below',
          amount: '180',
          referenceAmount: '176.4',
          currency: 'USD',
        })
      )
    ).toEqual({
      what: 'watch_target_below',
      price: { value: 176.4, currency: 'USD' },
      target: { value: 180, currency: 'USD' },
      amount: null,
    });
    expect(agendaRowParts(m({ kind: 'watch_target', direction: 'above' })).what).toBe(
      'watch_target_above'
    );
  });

  it('names a split by its ratio and counts the trades to adjust', () => {
    expect(
      agendaRowParts(
        m({ kind: 'stock_split', direction: '4:1' as Milestone['direction'], count: 3 })
      )
    ).toEqual({
      what: 'stock_split',
      ratio: '4:1',
      count: 3,
      amount: null,
    });
  });

  it('keeps the plain kinds as one word and nothing else', () => {
    for (const kind of [
      'insurance_end',
      'loan_fixation_end',
      'loan_payoff',
      'account_termination',
      'savings_rate_end',
    ] as const) {
      expect(agendaRowParts(m({ kind }))).toEqual({ what: kind, amount: null });
    }
  });

  it('falls back to the shared sub-line for a kind the phone has no word for', () => {
    expect(agendaRowParts(m({ kind: 'backup_stale' }))).toEqual({ what: null, amount: null });
  });
});
