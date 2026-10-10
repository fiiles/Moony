import { describe, expect, it } from 'vitest';
import type { FlowKind, FlowSource } from '@shared/schema';
import {
  preserveMoneyText,
  bookedInAccountCurrency,
  absoluteMovementShares,
  rowFlowLabel,
} from './money-flows';
import { completeAccountIdentifier } from './iban-utils';

describe('money flow presentation', () => {
  it('preserves original precision on no-edit and rounds new currency precision', () => {
    expect(preserveMoneyText('1000.12345', 'CZK', 1000.12345, 'CZK')).toBe('1000.12345');
    expect(preserveMoneyText(null, null, 1234.56, 'JPY')).toBe('1235');
    expect(preserveMoneyText(null, null, 1234.5678, 'KWD')).toBe('1234.568');
  });
  describe('bookedInAccountCurrency', () => {
    const DAY = 86400;
    const row = (id: string, currency: string, amount: string, status = 'booked') => ({
      id,
      currency,
      status,
      amount,
      bookingDate: 20_000 * DAY,
    });
    // 1 USD = 22 CZK, 1 EUR = 25 CZK; anything else has no rate
    const rates: Record<string, number> = { USD: 22, EUR: 25 };
    const convertAt = (amount: number, from: string) =>
      rates[from] === undefined ? null : amount * rates[from];

    it('keeps account-currency rows and drops pending ones', () => {
      const result = bookedInAccountCurrency(
        [row('a', 'CZK', '100'), row('b', 'CZK', '50', 'pending')],
        'CZK',
        convertAt
      );
      expect(result.rows).toEqual([row('a', 'CZK', '100')]);
      expect(result.converted).toEqual([]);
      expect(result.missing).toEqual([]);
    });

    it('converts foreign rows at their day into the account currency', () => {
      const result = bookedInAccountCurrency(
        [row('a', 'CZK', '17.65'), row('b', 'USD', '4.65'), row('c', 'EUR', '0.73')],
        'CZK',
        convertAt
      );
      expect(result.rows.map((r) => [r.id, r.currency])).toEqual([
        ['a', 'CZK'],
        ['b', 'CZK'],
        ['c', 'CZK'],
      ]);
      expect(Number(result.rows[0].amount)).toBe(17.65);
      expect(Number(result.rows[1].amount)).toBeCloseTo(102.3, 9);
      expect(Number(result.rows[2].amount)).toBeCloseTo(18.25, 9);
      expect(result.converted).toEqual(['EUR', 'USD']);
      expect(result.missing).toEqual([]);
    });

    it('leaves out and reports rows without a rate for their day', () => {
      const result = bookedInAccountCurrency(
        [row('a', 'CZK', '10'), row('b', 'KWD', '3'), row('c', 'KWD', '1')],
        'CZK',
        convertAt
      );
      expect(result.rows.map((r) => r.id)).toEqual(['a']);
      expect(result.converted).toEqual([]);
      expect(result.missing).toEqual(['KWD']);
      expect(result.missingCount).toBe(2);
    });

    it('passes the booking day to the converter', () => {
      const seen: number[] = [];
      bookedInAccountCurrency([row('b', 'USD', '1')], 'CZK', (amount, _from, timestamp) => {
        seen.push(timestamp);
        return amount;
      });
      expect(seen).toEqual([20_000 * DAY]);
    });
  });
  it('keeps signed amounts while labeling absolute movement shares', () => {
    expect(absoluteMovementShares([1000, -500])).toEqual([2 / 3, 1 / 3]);
    expect(absoluteMovementShares([0])).toEqual([0]);
  });
  it('prefers complete IBAN over bare BBAN and preserves routing text', () => {
    expect(completeAccountIdentifier('CZ6508000000192000145399', '19-2000145399')).toBe(
      '19-2000145399/0800'
    );
    expect(completeAccountIdentifier(null, '19-2000145399')).toBe('19-2000145399');
    expect(completeAccountIdentifier('123456789', null)).toBe('123456789');
    expect(completeAccountIdentifier('DE89370400440532013000', '123456789')).toBe(
      'DE89 3704 0044 0532 0130 00'
    );
  });
});

describe('ledger row flow label', () => {
  const flow = (effectiveFlowKind: FlowKind, flowSource: FlowSource = 'category') => ({
    effectiveFlowKind,
    flowSource,
  });

  it('stays silent when the sign or the category already says it', () => {
    expect(rowFlowLabel(flow('expense', 'direction'), false, false)).toBeNull();
    expect(rowFlowLabel(flow('income', 'direction'), true, false)).toBeNull();
    expect(rowFlowLabel(flow('transfer'), false, true)).toBeNull();
    expect(rowFlowLabel(flow('transfer', 'ownAccount'), false, true)).toBeNull();
  });

  it('names refunds, explicit choices and uncategorized own-account transfers', () => {
    expect(rowFlowLabel(flow('expense'), true, true)).toBe('refund');
    expect(rowFlowLabel(flow('expense', 'explicit'), false, false)).toBe('expense');
    expect(rowFlowLabel(flow('transfer', 'ownAccount'), false, false)).toBe('transfer');
  });
});
