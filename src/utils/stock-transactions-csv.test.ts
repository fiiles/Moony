import { describe, expect, it } from 'vitest';
import type { StockReportLedgerRow } from '@shared/schema';
import { ledgerCsv, ledgerCsvFilename } from './stock-transactions-csv';

const base: StockReportLedgerRow = {
  id: 'r1',
  kind: 'buy',
  day: Date.UTC(2026, 0, 5) / 1000,
  investmentId: 'i1',
  ticker: 'AAPL',
  companyName: 'Apple, "the" Inc.',
  quantity: '2',
  price: '100.5',
  currency: 'USD',
  rate: '20',
  amount: '4020',
  fee: '1',
};

describe('ledgerCsv', () => {
  it('writes the header and quotes a name with a comma and a quote', () => {
    const lines = ledgerCsv([base], 'CZK').split('\n');
    expect(lines[0]).toBe(
      'Date,Type,Ticker,Name,Quantity,Price,Currency,Rate,Amount,Report currency,Fee'
    );
    expect(lines[1]).toBe('2026-01-05,Buy,AAPL,"Apple, ""the"" Inc.",2,100.5,USD,20,4020,CZK,1');
  });

  it('leaves null fields empty for a manual dividend', () => {
    const div: StockReportLedgerRow = {
      ...base,
      kind: 'dividend',
      companyName: 'Apple',
      quantity: null,
      price: null,
      fee: null,
    };
    expect(ledgerCsv([div], 'CZK').split('\n')[1]).toBe(
      '2026-01-05,Dividend,AAPL,Apple,,,USD,20,4020,CZK,'
    );
  });

  it('ends with a line break and has no BOM', () => {
    const csv = ledgerCsv([], 'CZK');
    expect(csv.endsWith('\n')).toBe(true);
    expect(csv.charCodeAt(0)).not.toBe(0xfeff);
  });
});

describe('ledgerCsvFilename', () => {
  it('names the period', () => {
    expect(ledgerCsvFilename(Date.UTC(2026, 0, 1) / 1000, Date.UTC(2026, 11, 31) / 1000)).toBe(
      'stock_transactions_2026-01-01_2026-12-31.csv'
    );
  });
});
