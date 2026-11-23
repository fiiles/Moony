import type { StockReportLedgerRow } from '@shared/schema';

const HEADER = [
  'Date',
  'Type',
  'Ticker',
  'Name',
  'Quantity',
  'Price',
  'Currency',
  'Rate',
  'Amount',
  'Report currency',
  'Fee',
];

const KIND_LABEL = { buy: 'Buy', sell: 'Sell', dividend: 'Dividend' } as const;

function isoDay(day: number): string {
  return new Date(day * 1000).toISOString().slice(0, 10);
}

/** RFC 4180: quote a field holding a comma, a quote or a line break; double the quotes. */
function field(value: string | null): string {
  if (value === null) return '';
  return /[",\r\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
}

/** The ledger as CSV text, `\n` line ends, no BOM (the caller adds it). */
export function ledgerCsv(rows: StockReportLedgerRow[], reportCurrency: string): string {
  const lines = [HEADER.join(',')];
  for (const row of rows) {
    lines.push(
      [
        isoDay(row.day),
        KIND_LABEL[row.kind],
        row.ticker,
        row.companyName,
        row.quantity,
        row.price,
        row.currency,
        row.rate,
        row.amount,
        reportCurrency,
        row.fee,
      ]
        .map(field)
        .join(',')
    );
  }
  return lines.join('\n') + '\n';
}

export function ledgerCsvFilename(fromDay: number, toDay: number): string {
  return `stock_transactions_${isoDay(fromDay)}_${isoDay(toDay)}.csv`;
}
