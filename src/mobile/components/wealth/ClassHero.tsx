import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { usePortfolioHistory } from '@/hooks/use-portfolio';
import { useFormat } from '@/lib/use-format';
import type { EventCluster } from '@/utils/chart-scale';
import { CHART_PERIODS, chartPeriodStart, type ChartPeriod } from '@/utils/period';
import {
  firstTradeDay,
  tickerSummary,
  tradeDayEvents,
  type TradeDayEvent,
  type TradeLike,
} from '@/utils/trade-events';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { Hero } from '@/mobile/components/Hero';
import { Money } from '@/mobile/components/Money';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useRemembered } from '@/mobile/hooks/use-remembered';
import { changeSinceDayOptions } from '@/mobile/lib/change';
import {
  classTrendPoints,
  pointsChange,
  trendChangeBase,
  type TrendClass,
} from '@/mobile/lib/class-trend';

/** Whole amounts: the class page shows no decimals. */
const WHOLE = { decimals: 0 } as const;

interface ClassHeroProps {
  /** Which part of the portfolio history the line draws. */
  trendClass: TrendClass;
  /** The hero's label and the chart's accessible name. */
  label: string;
  /** Today's value of the class in the report currency; `null` when it could not be valued. */
  totalValue: number | null;
  /** What is invested, said after the change; `null` leaves it out. */
  invested: number | null;
  currency: string;
  /** Every trade of the class: with them the line carries a mark per buy and sell day and a legend. */
  trades?: readonly (TradeLike & { transactionDate: number })[];
  /** Converts a trade at the rates of its own day; `null` without a rate (the mark then has no amount). */
  convertAt?: (amount: number, from: string, to: string, date: number) => number | null;
  /** The period is remembered for the session under `${rememberKey}.period`. */
  rememberKey: string;
}

/**
 * The value of a class (spec §8.8): the line of the period with today's value as its last point
 * and, when the trades are given, the buy and sell days as marks (a tap opens the day), the change
 * over the period and what is invested. The legend names the marks.
 */
export function ClassHero({
  trendClass,
  label,
  totalValue,
  invested,
  currency,
  trades,
  convertAt,
  rememberKey,
}: ClassHeroProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: td } = useTranslation('dashboard');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const [period, setPeriod] = useRemembered<ChartPeriod>(`${rememberKey}.period`, '30D');

  // No horizon starts before the first trade; the start is a whole UTC day, so the query holds still
  const earliest = useMemo(() => firstTradeDay(trades ?? []), [trades]);
  const start = chartPeriodStart(period, Date.now() / 1000, earliest);
  const { data: history } = usePortfolioHistory(start);

  const points = useMemo(
    () => classTrendPoints(history, trendClass, currency, totalValue, Date.now() / 1000),
    [history, trendClass, currency, totalValue]
  );
  const change = pointsChange(points);
  // "over 30 days", or "vs 3. 10." when the line starts later than the period (a young portfolio):
  // the period's own start, not the one clamped to the first trade
  const changeBase = trendChangeBase(points, chartPeriodStart(period, Date.now() / 1000));
  const changeLabel =
    changeBase === null
      ? t(`overview.change.${period}`)
      : td('changeSince', { date: fmt.day(changeBase, changeSinceDayOptions(changeBase)) });

  // Every trade at its own day's rate, like the history line it sits on; events before the horizon
  // would pile up on the left edge
  const events = useMemo(
    () =>
      trades === undefined
        ? undefined
        : tradeDayEvents(
            trades,
            (amount, from, date) => convertAt?.(amount, from, currency, date) ?? null
          ).filter((event) => start === undefined || event.t >= start),
    [trades, convertAt, currency, start]
  );
  const eventTip = (cluster: EventCluster<TradeDayEvent>) => {
    if (cluster.events.length > 1) {
      const first = cluster.events[0].t;
      const last = cluster.events[cluster.events.length - 1].t;
      return {
        title: ts('chart.events.cluster', { count: cluster.events.length }),
        lines: [`${fmt.day(first)} – ${fmt.day(last)}`],
      };
    }
    const event = cluster.events[0];
    return {
      title: ts(event.type === 'sell' ? 'chart.events.sell' : 'chart.events.buy', {
        tickers: tickerSummary(event.tickers),
      }),
      lines: [
        event.amount === null
          ? fmt.day(event.t)
          : `${fmt.day(event.t)} · ${moneyText(event.amount, currency, WHOLE)}`,
      ],
    };
  };

  return (
    <Hero
      label={label}
      value={totalValue}
      formatValue={(value) => moneyText(value, currency, WHOLE)}
      change={
        change
          ? {
              ratio: change.ratio ?? 0,
              text: (
                <>
                  <Money
                    value={change.delta}
                    currency={currency}
                    options={{ signed: true, ...WHOLE }}
                  />{' '}
                  {changeLabel}
                  {invested !== null && (
                    <>
                      {' · '}
                      {t('wealth.stocks.invested', {
                        amount: moneyText(invested, currency, WHOLE),
                      })}
                    </>
                  )}
                </>
              ),
            }
          : undefined
      }
      note={t('overview.stats.noHistory')}
      period={period}
      periods={CHART_PERIODS}
      onPeriodChange={setPeriod}
      points={points}
      zeroBaseline={period === 'All'}
      events={events}
      renderEventTip={trades === undefined ? undefined : eventTip}
      chartLabel={label}
    >
      {trades !== undefined && points.length > 1 && (
        <ChartLegend
          items={[
            { label: ts('chart.legend.value'), swatch: { kind: 'line' } },
            { label: ts('chart.legend.buy'), swatch: { kind: 'event', type: 'buy' } },
            { label: ts('chart.legend.sell'), swatch: { kind: 'event', type: 'sell' } },
          ]}
        />
      )}
    </Hero>
  );
}
