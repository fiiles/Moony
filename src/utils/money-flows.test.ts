import { describe, expect, it } from 'vitest';
import type { FlowKind, FlowSource } from '@shared/schema';
import {
  preserveMoneyText,
  nativeBookedTransactions,
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
  it('excludes pending and mismatched units from native booked sums', () => {
    const rows = [
      { currency: 'CZK', status: 'booked' },
      { currency: 'EUR', status: 'booked' },
      { currency: 'CZK', status: 'pending' },
    ];
    expect(nativeBookedTransactions(rows, 'CZK')).toEqual([rows[0]]);
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
    expect(rowFlowLabel(flow('investment'), false, true)).toBeNull();
    expect(rowFlowLabel(flow('transfer', 'ownAccount'), false, true)).toBeNull();
  });

  it('names refunds, explicit choices and uncategorized own-account transfers', () => {
    expect(rowFlowLabel(flow('expense'), true, true)).toBe('refund');
    expect(rowFlowLabel(flow('expense', 'explicit'), false, false)).toBe('expense');
    expect(rowFlowLabel(flow('transfer', 'ownAccount'), false, false)).toBe('transfer');
  });
});
