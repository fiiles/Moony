import { describe, expect, it } from 'vitest';
import type { FlowKind, ReportTransaction } from '@shared/schema';
import { DRILLDOWN_PAGE, drilldownRows } from './cashflow-drilldown';

function tx(patch: Partial<ReportTransaction> = {}): ReportTransaction {
  return {
    reportCurrency: 'CZK',
    effectiveFlowKind: 'expense',
    flowSource: 'category',
    signedAmount: '-250',
    fxQuality: 'historical',
    id: 'tx_1',
    bookingDate: 1_760_000_000,
    amount: '250',
    currency: 'CZK',
    description: 'Nákup potravin',
    remittanceInfo: null,
    counterpartyName: null,
    counterpartyIban: null,
    counterpartyBankCode: null,
    counterpartyCountry: null,
    categoryId: 'cat_x',
    bankAccountId: 'acc_1',
    bankAccountName: 'Běžný účet',
    txType: 'debit',
    ...patch,
  };
}

const NO_CHANGES: ReadonlyMap<string, string | null> = new Map();
const day = (bookingDate: number) => `day ${bookingDate}`;

describe('drilldownRows', () => {
  it('titles a row by its counterparty and adds the first detail after the account', () => {
    const [row] = drilldownRows(
      [
        tx({
          counterpartyName: 'Albert',
          description: 'Nákup potravin',
          remittanceInfo: 'Paragon 118',
        }),
      ],
      NO_CHANGES,
      'cat_x',
      day
    );
    expect(row.title).toBe('Albert');
    expect(row.sub).toEqual([`day ${tx().bookingDate}`, 'Běžný účet', 'Nákup potravin']);
  });

  it('titles a row without a name by the counterparty account', () => {
    const [row] = drilldownRows(
      [tx({ counterpartyIban: 'CZ6508000000192000145399' })],
      NO_CHANGES,
      'cat_x',
      day
    );
    expect(row.title).toBe('19-2000145399/0800');
    expect(row.sub).toEqual([`day ${tx().bookingDate}`, 'Běžný účet', 'Nákup potravin']);
  });

  it('titles a row without a counterparty by its description, with no detail in the sub', () => {
    const [row] = drilldownRows(
      [tx({ description: 'Platba kartou', remittanceInfo: 'Terminal 4' })],
      NO_CHANGES,
      'cat_x',
      day
    );
    expect(row.title).toBe('Platba kartou');
    expect(row.sub).toEqual([`day ${tx().bookingDate}`, 'Běžný účet']);
  });

  it('falls back to the account name when the row has nothing else to say', () => {
    const [row] = drilldownRows(
      [tx({ description: null, remittanceInfo: null })],
      NO_CHANGES,
      'cat_x',
      day
    );
    expect(row.title).toBe('Běžný účet');
    expect(row.sub).toEqual([`day ${tx().bookingDate}`, 'Běžný účet']);
  });

  it('reads the credit from the transaction type', () => {
    const rows = drilldownRows(
      [tx({ id: 'a', txType: 'credit' }), tx({ id: 'b', txType: 'debit' })],
      NO_CHANGES,
      'cat_x',
      day
    );
    expect(rows.map((row) => row.credit)).toEqual([true, false]);
  });

  it('marks a row changed to another category as moved and reports the new category', () => {
    const [row] = drilldownRows([tx()], new Map([['tx_1', 'cat_y']]), 'cat_x', day);
    expect(row.moved).toBe(true);
    expect(row.categoryId).toBe('cat_y');
  });

  it('does not mark a row moved that was changed back to the target', () => {
    const [row] = drilldownRows([tx()], new Map([['tx_1', 'cat_x']]), 'cat_x', day);
    expect(row.moved).toBe(false);
    expect(row.categoryId).toBe('cat_x');
  });

  it('keeps the loaded category of a row that was never changed', () => {
    const [row] = drilldownRows([tx({ categoryId: 'cat_z' })], NO_CHANGES, 'cat_x', day);
    expect(row.moved).toBe(false);
    expect(row.categoryId).toBe('cat_z');
  });

  it('moves every changed row out of the uncategorized list', () => {
    const [row] = drilldownRows(
      [tx({ categoryId: null })],
      new Map([['tx_1', 'cat_y']]),
      'uncategorized',
      day
    );
    expect(row.moved).toBe(true);
    expect(row.categoryId).toBe('cat_y');
  });

  describe('the meaning of the payment', () => {
    const expense = (current: ReadonlyMap<string, FlowKind>) => ({
      flowKind: 'expense' as const,
      current,
    });

    it('marks a row moved whose effective flow left the list, with no category change', () => {
      const [row] = drilldownRows(
        [tx()],
        NO_CHANGES,
        'cat_x',
        day,
        expense(new Map([['tx_1', 'transfer']]))
      );
      expect(row.moved).toBe(true);
      expect(row.categoryId).toBe('cat_x');
    });

    it('does not mark a row moved whose effective flow is still the list’s', () => {
      const [row] = drilldownRows(
        [tx()],
        NO_CHANGES,
        'cat_x',
        day,
        expense(new Map([['tx_1', 'expense']]))
      );
      expect(row.moved).toBe(false);
    });

    it('leaves a row alone that is in neither map', () => {
      const [row] = drilldownRows(
        [tx()],
        NO_CHANGES,
        'cat_x',
        day,
        expense(new Map([['other', 'transfer']]))
      );
      expect(row.moved).toBe(false);
    });

    it('keeps a category move moved when the flow is still the list’s', () => {
      const [row] = drilldownRows(
        [tx()],
        new Map([['tx_1', 'cat_y']]),
        'cat_x',
        day,
        expense(new Map([['tx_1', 'expense']]))
      );
      expect(row.moved).toBe(true);
    });

    it('reads the rule against the list’s own flow, whichever it is', () => {
      const rows = drilldownRows(
        [tx({ id: 'in', effectiveFlowKind: 'income' })],
        NO_CHANGES,
        'cat_x',
        day,
        { flowKind: 'income', current: new Map([['in', 'income']]) }
      );
      expect(rows[0].moved).toBe(false);
      const moved = drilldownRows([tx({ id: 'in' })], NO_CHANGES, 'cat_x', day, {
        flowKind: 'income',
        current: new Map([['in', 'expense']]),
      });
      expect(moved[0].moved).toBe(true);
    });

    it('applies the category rule alone when no flows are given', () => {
      const rows = drilldownRows(
        [tx({ id: 'a' }), tx({ id: 'b' })],
        new Map([['a', 'cat_y']]),
        'cat_x',
        day
      );
      expect(rows.map((row) => row.moved)).toEqual([true, false]);
    });
  });

  describe('reportAmount', () => {
    it('is the converted amount of a foreign expense debit, negative like the row', () => {
      // signedAmount is the row's contribution to its report line: an expense debit is positive
      const [row] = drilldownRows(
        [tx({ txType: 'debit', currency: 'EUR', amount: '50', signedAmount: '1250' })],
        NO_CHANGES,
        'cat_x',
        day
      );
      expect(row.reportAmount).toBe(-1250);
    });

    it('is positive for a refund (a credit in an expense list, negative in the line)', () => {
      const [row] = drilldownRows(
        [tx({ txType: 'credit', currency: 'EUR', amount: '20', signedAmount: '-500' })],
        NO_CHANGES,
        'cat_x',
        day
      );
      expect(row.reportAmount).toBe(500);
    });

    it('is positive for income credits and negative for a transfer debit', () => {
      const rows = drilldownRows(
        [
          tx({
            id: 'in',
            txType: 'credit',
            effectiveFlowKind: 'income',
            currency: 'EUR',
            signedAmount: '800',
          }),
          tx({
            id: 'out',
            txType: 'debit',
            effectiveFlowKind: 'transfer',
            currency: 'EUR',
            signedAmount: '-2000',
          }),
        ],
        NO_CHANGES,
        'cat_x',
        day
      );
      expect(rows.map((row) => row.reportAmount)).toEqual([800, -2000]);
    });

    it('is null when the row has no converted amount (no rate)', () => {
      const [row] = drilldownRows([tx({ signedAmount: null })], NO_CHANGES, 'cat_x', day);
      expect(row.reportAmount).toBeNull();
    });
  });

  it('shows the day through the label the caller gives', () => {
    const [row] = drilldownRows([tx({ bookingDate: 1234 })], NO_CHANGES, 'cat_x', day);
    expect(row.sub[0]).toBe('day 1234');
  });

  it('pages by a hundred rows', () => {
    expect(DRILLDOWN_PAGE).toBe(100);
  });
});
