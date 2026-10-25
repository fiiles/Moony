import { useTranslation } from 'react-i18next';
import { priceDecimals } from '@shared/currencies';
import { useFormat } from '@/lib/use-format';
import type { EventCluster } from '@/utils/chart-scale';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { nativeTotal, type LedgerTx, type PositionEvent } from '@/mobile/lib/position-series';

/** What a chart mark's callout says: a title and the lines under it. */
export interface TradeTip {
  title: string;
  lines: string[];
}

/**
 * The callout of a trade mark on a holding's chart (a position's and an other asset's hero): one
 * trade reads "Nákup 5 ks za 182,40 $" over "3. 10. 2026 · celkem 912 $" (in the trade's own
 * currency, `fallbackCurrency` when it has none); a cluster of marks reads "3 transakce" over its
 * first and last day. Amounts go through the eye. `unit` is what the quantity is counted in
 * ("ks"), `quantityDigits` the decimals it may show.
 */
export function useTradeEventTip<T extends LedgerTx>(
  unit: string,
  quantityDigits: number,
  fallbackCurrency: string
): (cluster: EventCluster<PositionEvent<T>>) => TradeTip {
  const { t: ts } = useTranslation('stocks');
  const fmt = useFormat();
  const moneyText = useMoneyText();

  return (cluster) => {
    if (cluster.events.length > 1) {
      const first = cluster.events[0].t;
      const last = cluster.events[cluster.events.length - 1].t;
      return {
        title: ts('detail.events.cluster', { count: cluster.events.length }),
        lines: [`${fmt.day(first)} – ${fmt.day(last)}`],
      };
    }
    const { tx } = cluster.events[0];
    const price = parseFloat(tx.pricePerUnit) || 0;
    const txCurrency = tx.currency || fallbackCurrency;
    return {
      title: ts(tx.type === 'sell' ? 'detail.events.sell' : 'detail.events.buy', {
        quantity: `${fmt.number(parseFloat(tx.quantity) || 0, { maximumFractionDigits: quantityDigits })} ${unit}`,
        price: moneyText(price, txCurrency, { decimals: priceDecimals(price, txCurrency) }),
      }),
      lines: [
        `${fmt.day(tx.transactionDate)} · ${ts('detail.events.total', {
          amount: moneyText(nativeTotal(tx), txCurrency),
        })}`,
      ],
    };
  };
}
