import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { priceDecimals } from '@shared/currencies';
import type { InvestmentTransaction, TickerValueHistoryReport } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import type { EventCluster } from '@/utils/chart-scale';
import { CHART_PERIODS, chartPeriodStart, type ChartPeriod } from '@/utils/period';
import type { StockRow } from '@/utils/positions';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { Hero } from '@/mobile/components/Hero';
import { Money } from '@/mobile/components/Money';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { changeSinceDayOptions } from '@/mobile/lib/change';
import { pointsChange, trendChangeBase } from '@/mobile/lib/class-trend';
import {
  nativeTotal,
  positionEvents,
  positionPoints,
  type PositionEvent,
} from '@/mobile/lib/position-series';

/** Whole amounts: the hero's change and axis show no decimals. */
const WHOLE = { decimals: 0 } as const;

interface PositionHeroCardProps {
  holding: StockRow;
  /** The value history of the period, each day in the report currency. */
  history: TickerValueHistoryReport | undefined;
  transactions: readonly InvestmentTransaction[];
  reportCurrency: string;
  period: ChartPeriod;
  onPeriodChange: (period: ChartPeriod) => void;
  /** A tap on a mark opened its callout: the ids of the trades it stands for (their rows flash). */
  onMarkTap: (ids: readonly string[]) => void;
}

/**
 * The value of the position (spec §8.8, prototype frame 12): today's value and the quantity in the
 * label, the line of the period with the dashed "Vloženo" cost line and a mark for every buy and
 * sell (a tap opens the callout and flashes the matching rows), the change over the period with the
 * price per unit in the stock's own currency. The line is the desktop hero's: each day's value and
 * cost basis from the backend, today's figures as its last point.
 */
export function PositionHeroCard({
  holding,
  history,
  transactions,
  reportCurrency,
  period,
  onPeriodChange,
  onMarkTap,
}: PositionHeroCardProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: td } = useTranslation('dashboard');
  const fmt = useFormat();
  const moneyText = useMoneyText();

  const { marketValue, totalCost, quantity, originalCurrentPrice, originalCurrency } = holding;
  const points = useMemo(
    () => positionPoints(history, reportCurrency, marketValue, totalCost, Date.now() / 1000),
    [history, reportCurrency, marketValue, totalCost]
  );
  const change = pointsChange(points);

  // "over 30 days", or "vs 3. 10." when the line starts later than the period (a young position)
  const periodStart = chartPeriodStart(period, Date.now() / 1000);
  const changeBase = trendChangeBase(points, periodStart);
  const changeLabel =
    changeBase === null
      ? t(`overview.change.${period}`)
      : td('changeSince', { date: fmt.day(changeBase, changeSinceDayOptions(changeBase)) });
  const perUnit =
    originalCurrentPrice > 0
      ? t('wealth.position.perUnit', {
          price: moneyText(originalCurrentPrice, originalCurrency, {
            decimals: priceDecimals(originalCurrentPrice, originalCurrency),
          }),
        })
      : null;

  // Marks before the horizon would pile up on the left edge
  const events = useMemo(
    () =>
      positionEvents(transactions).filter((e) => periodStart === undefined || e.t >= periodStart),
    [transactions, periodStart]
  );
  const eventTip = (cluster: EventCluster<PositionEvent>) => {
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
    const txCurrency = tx.currency || reportCurrency;
    return {
      title: ts(tx.type === 'sell' ? 'detail.events.sell' : 'detail.events.buy', {
        quantity: `${fmt.number(parseFloat(tx.quantity) || 0, { maximumFractionDigits: 4 })} ${ts('table.unit')}`,
        price: moneyText(price, txCurrency, { decimals: priceDecimals(price, txCurrency) }),
      }),
      lines: [
        `${fmt.day(tx.transactionDate)} · ${ts('detail.events.total', {
          amount: moneyText(nativeTotal(tx), txCurrency),
        })}`,
      ],
    };
  };

  const label = t('wealth.position.heroLabel', {
    quantity: fmt.number(quantity, { maximumFractionDigits: 4 }),
    unit: ts('table.unit'),
  });
  const missing = history?.fxQuality.missingCurrencies ?? [];

  return (
    <Hero
      label={label}
      value={marketValue}
      formatValue={(value) => moneyText(value, reportCurrency, WHOLE)}
      change={
        change
          ? {
              ratio: change.ratio ?? 0,
              text: (
                <>
                  <Money
                    value={change.delta}
                    currency={reportCurrency}
                    options={{ signed: true, ...WHOLE }}
                  />{' '}
                  {changeLabel}
                  {perUnit && ` · ${perUnit}`}
                </>
              ),
            }
          : undefined
      }
      note={t('overview.stats.noHistory')}
      period={period}
      periods={CHART_PERIODS}
      onPeriodChange={onPeriodChange}
      points={points}
      cost
      zeroBaseline={period === 'All'}
      events={events}
      renderEventTip={eventTip}
      onMarkTap={(cluster) => onMarkTap(cluster.events.map((e) => e.id))}
      chartLabel={label}
    >
      {points.length > 1 && (
        <ChartLegend
          items={[
            { label: ts('detail.legend.value'), swatch: { kind: 'line' } },
            { label: ts('detail.legend.investedLong'), swatch: { kind: 'dash' } },
            { label: ts('detail.legend.buy'), swatch: { kind: 'event', type: 'buy' } },
            { label: ts('detail.legend.sell'), swatch: { kind: 'event', type: 'sell' } },
          ]}
        />
      )}
      {missing.length > 0 && (
        <p className="mb-0 mt-2 text-m-note font-semibold text-loss" role="note">
          {ts('detail.chartFxMissing', { currencies: missing.join(', ') })}
        </p>
      )}
    </Hero>
  );
}
