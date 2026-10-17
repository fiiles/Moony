import { describe, expect, it } from 'vitest';
import type { ReportTransaction } from '@shared/schema';
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

  it('shows the day through the label the caller gives', () => {
    const [row] = drilldownRows([tx({ bookingDate: 1234 })], NO_CHANGES, 'cat_x', day);
    expect(row.sub[0]).toBe('day 1234');
  });

  it('pages by a hundred rows', () => {
    expect(DRILLDOWN_PAGE).toBe(100);
  });
});
