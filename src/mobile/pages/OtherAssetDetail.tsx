import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useRoute } from 'wouter';
import { ArrowUp, Minus, Plus } from 'lucide-react';
import { priceDecimals, recordCurrency } from '@shared/currencies';
import type { AssetValuation, OtherAsset, OtherAssetTransaction } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import type { EventCluster } from '@/utils/chart-scale';
import type { OtherAssetRow } from '@/utils/other-assets';
import { CHART_PERIODS, chartPeriodStart, type ChartPeriod } from '@/utils/period';
import { Button } from '@/components/ui/button';
import { ScaledChartLegend } from '@/mobile/components/Scaled';
import { BuyOtherAssetModal } from '@/components/other-assets/BuyOtherAssetModal';
import { SellOtherAssetModal } from '@/components/other-assets/SellOtherAssetModal';
import { ButtonPair } from '@/mobile/components/ButtonPair';
import { Hero } from '@/mobile/components/Hero';
import { Money } from '@/mobile/components/Money';
import { StatRow, type StatItem } from '@/mobile/components/StatRow';
import { ClassSkeleton } from '@/mobile/components/wealth/ClassSkeleton';
import { OtherAssetMenu } from '@/mobile/components/wealth/OtherAssetMenu';
import { PositionLedger } from '@/mobile/components/wealth/PositionLedger';
import { useBack } from '@/mobile/hooks/use-back';
import { useDeleteAndLeave } from '@/mobile/hooks/use-delete-and-leave';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useOtherAsset, useOtherAssetMutations } from '@/mobile/hooks/use-other-asset';
import { usePositionLedger } from '@/mobile/hooks/use-position-ledger';
import { useRemembered } from '@/mobile/hooks/use-remembered';
import { useRetained } from '@/mobile/hooks/use-retained';
import { changeSinceDayOptions } from '@/mobile/lib/change';
import { pointsChange, trendChangeBase } from '@/mobile/lib/class-trend';
import { otherAssetTrace, trimTrace } from '@/mobile/lib/other-asset-trace';
import {
  nativeTotal,
  positionEvents,
  type LedgerTx,
  type PositionEvent,
} from '@/mobile/lib/position-series';
import { statMoneyOptions, statTone } from '@/mobile/lib/stat-money';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { otherAssetPath, routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { RevalueSheet } from '@/mobile/sheets/RevalueSheet';

/** What the screen shows of a loaded asset: the record and the row derived from it. */
interface AssetView {
  asset: OtherAsset;
  row: OtherAssetRow;
}

/** A quantity of an asset is held to eight decimals, as the desktop's transactions list writes it. */
const QUANTITY_DIGITS = 8;
/** Whole amounts: the hero's change shows no decimals. */
const WHOLE = { decimals: 0 } as const;

/** A trade as the ledger lists it: an asset has a name where a stock has a ticker. */
type AssetTrade = OtherAssetTransaction & LedgerTx;

/**
 * An other asset (spec §8.8, "Ostatní aktivum": the light detail): the name in the nav bar with
 * the quantity and the currency of its price under it, the value with its step line, Koupit ·
 * Prodat with Přecenit full-width under them, three stats and the transactions with their segment.
 * There is no company card and no market feed: the price is the person's own estimate. The hero is
 * the asset's native value in its price currency (ADR 0012, ruling 6); the numbers are the
 * desktop's (`otherAssetRow`, the same queries); the buy, sell, edit and valuation-history dialogs
 * are the desktop's, Přecenit is the phone's own sheet (`RevalueSheet`, the property's too). A
 * trade has no Upravit (there is no update command), only Smazat.
 *
 * The page owns the delete (its confirm sheet and the mutation, `useDeleteAndLeave`): the asset
 * drops out of the list the page reads while the delete is still finishing, which would take the
 * `···` menu away, and the page keeps showing the asset until it has left.
 */
export default function OtherAssetDetail() {
  const { t: to } = useTranslation('otherAssets');
  const { t: td } = useTranslation('dashboard');
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const [, params] = useRoute(routes.otherAssetDetail);
  const back = useBack(routes.otherAssetsList);
  const [period, setPeriod] = useRemembered<ChartPeriod>('otherAsset.period', '30D');
  const [dialog, setDialog] = useState<'buy' | 'sell' | null>(null);
  const [revalueOpen, setRevalueOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const data = useOtherAsset(params?.id);
  const { asset, row } = data;
  const trades = useMemo<AssetTrade[]>(
    () => data.transactions.map((tx) => ({ ...tx, ticker: asset?.name ?? '' })),
    [data.transactions, asset?.name]
  );
  const ledger = usePositionLedger(trades);
  const { deleteTransaction, remove } = useOtherAssetMutations(params?.id);
  const current = useMemo<AssetView | null>(
    () => (!data.isLoading && asset && row ? { asset, row } : null),
    [data.isLoading, asset, row]
  );
  const kept = useRetained(current);
  const exit = useDeleteAndLeave({
    home: routes.otherAssetsList,
    pathOf: otherAssetPath,
    keysOf: (assetId) => [
      ['other-asset-transactions', assetId],
      ['other-asset-valuations', assetId],
    ],
    remove: () => remove.mutateAsync(),
    closeSheet: () => setDeleteOpen(false),
  });
  // An asset being deleted stays on screen (as it was) until the screen has left
  const view = exit.deleting ? kept : current;

  return (
    <Screen
      title={view?.asset.name ?? ''}
      leading={<NavTextButton label={td('cards.otherAssets')} onClick={back} chevron />}
      trailing={
        view && (
          <OtherAssetMenu
            asset={view.asset}
            valuations={data.valuations}
            onDelete={() => setDeleteOpen(true)}
          />
        )
      }
    >
      {view ? (
        <>
          {/* The nav bar keeps its one title, so the quantity and the price currency are the first line */}
          <p className="mx-0.5 -mt-0.5 mb-2.5 truncate text-center text-m-caption text-ink-3">
            {[
              `${fmt.number(view.row.quantity, { maximumFractionDigits: QUANTITY_DIGITS })} ${to('table.unit')}`,
              view.row.priceCurrency,
            ].join(' · ')}
          </p>
          <AssetHero
            row={view.row}
            valuations={data.valuations}
            trades={trades}
            period={period}
            onPeriodChange={setPeriod}
            onMarkTap={ledger.flash}
          />
          <ButtonPair
            className="mb-2.5"
            primary={{ label: to('actions.buy'), icon: Plus, onClick: () => setDialog('buy') }}
            secondary={{
              label: to('actions.sell'),
              icon: Minus,
              onClick: () => setDialog('sell'),
              disabled: view.row.quantity <= 0,
            }}
          />
          <Button
            variant="secondary"
            className="mb-3.5 h-auto min-h-12 w-full whitespace-normal rounded-[10px] py-2 text-m-row [&_svg]:size-4"
            onClick={() => setRevalueOpen(true)}
          >
            <ArrowUp strokeWidth={1.75} aria-hidden />
            {to('actions.revalue')}
          </Button>
          <AssetStats row={view.row} />
          <PositionLedger
            ledger={ledger}
            count={trades.length}
            currency={data.reportCurrency}
            convertAt={data.convertAt}
            ratesReady={data.ratesReady}
            unit={to('table.unit')}
            quantityDigits={QUANTITY_DIGITS}
            deleteTransaction={deleteTransaction}
          />
          <BuyOtherAssetModal
            asset={view.asset}
            open={dialog === 'buy'}
            onOpenChange={(open) => !open && setDialog(null)}
          />
          <SellOtherAssetModal
            asset={view.asset}
            open={dialog === 'sell'}
            onOpenChange={(open) => !open && setDialog(null)}
          />
          <RevalueSheet
            open={revalueOpen}
            onOpenChange={setRevalueOpen}
            kind="otherAsset"
            assetId={view.asset.id}
            name={view.asset.name}
            value={view.row.price}
            // The estimate continues in the newest valuation's currency
            currency={recordCurrency(view.asset.marketPriceCurrency)}
            unit={to('table.unit')}
            quantity={view.row.quantity}
          />
          <ConfirmSheet
            open={deleteOpen}
            onOpenChange={setDeleteOpen}
            title={to('confirmDelete.title')}
            description={to('confirmDelete.description', { name: view.asset.name })}
            confirmLabel={to('actions.delete')}
            isPending={remove.isPending}
            onConfirm={() => exit.run(view.asset.id)}
          />
        </>
      ) : data.isLoading ? (
        // The outline of a class page is the detail's too: the hero, the stats and a few rows
        <ClassSkeleton />
      ) : (
        <p className="mx-0.5 mt-8 text-center text-m-body text-ink-3">
          {t('wealth.otherAssets.notFound')}
        </p>
      )}
    </Screen>
  );
}

/**
 * The value of the asset (spec §8.8, ruling 6): today's `quantity × price` in the price currency
 * and the step line of the estimates (`otherAssetTrace`) trimmed to the period, with a mark for
 * every buy and sell (a tap opens the callout and flashes the matching rows) and the change over
 * the period. The line holds each estimate until the next one, so it is a step line; there is no
 * dashed cost line, the asset has no market to measure it against.
 */
function AssetHero({
  row,
  valuations,
  trades,
  period,
  onPeriodChange,
  onMarkTap,
}: {
  row: OtherAssetRow;
  valuations: readonly AssetValuation[];
  trades: readonly AssetTrade[];
  period: ChartPeriod;
  onPeriodChange: (period: ChartPeriod) => void;
  /** A tap on a mark opened its callout: the ids of the trades it stands for (their rows flash). */
  onMarkTap: (ids: readonly string[]) => void;
}) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: td } = useTranslation('dashboard');
  const { t: to } = useTranslation('otherAssets');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { priceCurrency, price, quantity, valueNative } = row;

  // "over 30 days", or "vs 3. 10." when the line starts later than the period (a young asset)
  const periodStart = chartPeriodStart(period, Date.now() / 1000);
  const points = useMemo(
    () =>
      trimTrace(
        otherAssetTrace(valuations, trades, priceCurrency, price, quantity, Date.now() / 1000),
        periodStart
      ),
    [valuations, trades, priceCurrency, price, quantity, periodStart]
  );
  const change = pointsChange(points);
  const changeBase = trendChangeBase(points, periodStart);
  const changeLabel =
    changeBase === null
      ? t(`overview.change.${period}`)
      : td('changeSince', { date: fmt.day(changeBase, changeSinceDayOptions(changeBase)) });

  // Marks before the horizon would pile up on the left edge
  const events = useMemo(
    () =>
      positionEvents(trades).filter((event) => periodStart === undefined || event.t >= periodStart),
    [trades, periodStart]
  );
  const eventTip = (cluster: EventCluster<PositionEvent<AssetTrade>>) => {
    if (cluster.events.length > 1) {
      const first = cluster.events[0].t;
      const last = cluster.events[cluster.events.length - 1].t;
      return {
        title: ts('detail.events.cluster', { count: cluster.events.length }),
        lines: [`${fmt.day(first)} – ${fmt.day(last)}`],
      };
    }
    const { tx } = cluster.events[0];
    const unitPrice = parseFloat(tx.pricePerUnit) || 0;
    const txCurrency = tx.currency || priceCurrency;
    return {
      title: ts(tx.type === 'sell' ? 'detail.events.sell' : 'detail.events.buy', {
        quantity: `${fmt.number(parseFloat(tx.quantity) || 0, { maximumFractionDigits: QUANTITY_DIGITS })} ${to('table.unit')}`,
        price: moneyText(unitPrice, txCurrency, { decimals: priceDecimals(unitPrice, txCurrency) }),
      }),
      lines: [
        `${fmt.day(tx.transactionDate)} · ${ts('detail.events.total', {
          amount: moneyText(nativeTotal(tx), txCurrency),
        })}`,
      ],
    };
  };

  const label = t('wealth.position.heroLabel', {
    quantity: fmt.number(quantity, { maximumFractionDigits: QUANTITY_DIGITS }),
    unit: to('table.unit'),
  });

  return (
    <Hero
      label={label}
      value={valueNative}
      formatValue={(value) => moneyText(value, priceCurrency, WHOLE)}
      change={
        change
          ? {
              ratio: change.ratio ?? 0,
              text: (
                <>
                  <Money
                    value={change.delta}
                    currency={priceCurrency}
                    options={{ signed: true, ...WHOLE }}
                  />{' '}
                  {changeLabel}
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
      step
      zeroBaseline={period === 'All'}
      events={events}
      renderEventTip={eventTip}
      onMarkTap={(cluster) => onMarkTap(cluster.events.map((event) => event.id))}
      chartLabel={label}
    >
      {points.length > 1 && (
        <ScaledChartLegend
          items={[
            { label: ts('detail.legend.value'), swatch: { kind: 'line' } },
            { label: ts('detail.legend.buy'), swatch: { kind: 'event', type: 'buy' } },
            { label: ts('detail.legend.sell'), swatch: { kind: 'event', type: 'sell' } },
          ]}
        />
      )}
    </Hero>
  );
}

/**
 * Three stats (spec §8.8): the quantity with the price per unit, the purchase average per unit with
 * what is invested in all, and the unrealized result against it. Everything is native: the
 * purchase average is in the asset's currency, the price and the result in the price currency, and
 * when the two differ (the currency was edited) there is no result to give — "—", never a
 * subtraction across currencies (ADR 0012).
 */
function AssetStats({ row }: { row: OtherAssetRow }) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { quantity, price, priceCurrency, currency, avgPurchase, costNative, gainNative } = row;

  const unitMoney = (amount: number, code: string) =>
    moneyText(amount, code, { decimals: priceDecimals(amount, code) });

  const stats: StatItem[] = [
    {
      label: tc('labels.quantity'),
      value: fmt.number(quantity, { maximumFractionDigits: QUANTITY_DIGITS }),
      note:
        price > 0
          ? t('wealth.position.perUnit', { price: unitMoney(price, priceCurrency) })
          : undefined,
    },
    {
      label: ts('table.avgCost'),
      value: avgPurchase > 0 ? unitMoney(avgPurchase, currency) : '—',
      note:
        costNative > 0
          ? t('wealth.stocks.invested', { amount: moneyText(costNative, currency, WHOLE) })
          : undefined,
    },
    {
      label: ts('detail.unrealized'),
      value:
        gainNative === null
          ? '—'
          : moneyText(gainNative, priceCurrency, {
              ...statMoneyOptions(gainNative),
              signed: true,
            }),
      tone: statTone(gainNative),
      note:
        gainNative !== null && costNative > 0
          ? ts('detail.vsInvestedPct', {
              percent: fmt.percent(gainNative / costNative, 1, { signed: true }),
            })
          : undefined,
    },
  ];
  return <StatRow stats={stats} />;
}
