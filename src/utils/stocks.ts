import type { StockRow } from '@/utils/positions';

/** A stock position row of the list and its trade dialogs (see `toStockRow`). */
export type HoldingData = StockRow;

export function getInstrumentIcon(ticker: string): string {
  const colors = [
    'bg-orange-500',
    'bg-gray-800',
    'bg-teal-600',
    'bg-black',
    'bg-blue-600',
    'bg-purple-600',
    'bg-red-600',
  ];
  const hash = ticker.split('').reduce((acc, char) => acc + char.charCodeAt(0), 0);
  return colors[hash % colors.length];
}
