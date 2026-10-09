import { useMemo, useState } from 'react';
import { useLocation } from 'wouter';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Plus, RefreshCw, TrendingUp, Upload } from 'lucide-react';
import type { InvestmentTransaction, StockPositions } from '@shared/schema';
import type { CurrencyCode } from '@shared/currencies';
import { exportApi, investmentsApi, priceApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useStockTagsByInvestment } from '@/hooks/use-stock-tags';
import { useDatedConvert } from '@/hooks/use-dated-convert';
import type { HoldingData } from '@/utils/stocks';
import { amountOf, percentOfCost, toStockRow, topPerformer } from '@/utils/positions';
import type { EventCluster } from '@/utils/chart-scale';
import {
  firstTradeDay,
  tickerSummary,
  tradeDayEvents,
  type TradeDayEvent,
} from '@/utils/trade-events';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/common/EmptyState';
import { ExportButton } from '@/components/common/ExportButton';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { StatSkeleton, Stats } from '@/components/common/Stat';
import PortfolioTrendCard from '@/components/common/PortfolioTrendCard';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { AddInvestmentModal } from '@/components/stocks/AddInvestmentModal';
import { BuyInvestmentModal } from '@/components/stocks/BuyInvestmentModal';
import { SellInvestmentModal } from '@/components/stocks/SellInvestmentModal';
import { ManualPriceModal } from '@/components/stocks/ManualPriceModal';
import { StockImportDialog } from '@/components/stocks/import/StockImportDialog';
import { InvestmentsSummary } from '@/components/stocks/InvestmentsSummary';
import { InvestmentsTable } from '@/components/stocks/InvestmentsTable';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';

type RowModal = 'buy' | 'sell' | 'price' | 'delete' | null;

export default function Stocks() {
  const tagsByInvestmentId = useStockTagsByInvestment();
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const { currencyCode, fxRevision, formatCurrency } = useCurrency();
  const fmt = useFormat();
  const [addOpen, setAddOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [rowModal, setRowModal] = useState<RowModal>(null);
  const [selected, setSelected] = useState<HoldingData | null>(null);

  const { data: positions, isLoading } = useQuery<StockPositions>({
    queryKey: ['investments', currencyCode, fxRevision],
    queryFn: () => investmentsApi.getAll(currencyCode),
    refetchOnMount: true,
    staleTime: 60 * 1000,
  });

  const { data: allTransactions } = useQuery<InvestmentTransaction[]>({
    queryKey: ['all-stock-transactions'],
    queryFn: () => investmentsApi.getAllTransactions(),
    staleTime: 60 * 1000,
  });

  // Date-aware conversion for the trade markers of the trend card, which still plots the
  // CZK history (its amounts convert at their own day's rates, ADR 0001).
  const convertAt = useDatedConvert(allTransactions);

  // No horizon of the trend card starts before the first transaction
  const earliest = useMemo(() => firstTradeDay(allTransactions ?? []), [allTransactions]);

  // Buy and sell days as events on the aggregate trend, like the crypto list:
  // one marker per day and direction, listing the tickers and the day's total.
  const trendEvents = useMemo(
    () =>
      tradeDayEvents(allTransactions ?? [], (amount, currency, date) =>
        convertAt(amount, currency as CurrencyCode, 'CZK', date)
      ),
    [allTransactions, convertAt]
  );

  const trendEventTip = (cluster: EventCluster<TradeDayEvent>) => {
    if (cluster.events.length > 1) {
      const first = cluster.events[0].t;
      const last = cluster.events[cluster.events.length - 1].t;
      return {
        title: t('chart.events.cluster', { count: cluster.events.length }),
        lines: [`${fmt.day(first)} – ${fmt.day(last)}`],
      };
    }
    const event = cluster.events[0];
    return {
      title: t(event.type === 'sell' ? 'chart.events.sell' : 'chart.events.buy', {
        tickers: tickerSummary(event.tickers),
      }),
      lines: [`${fmt.day(event.t)} · ${formatCurrency(event.amountCzk)}`],
    };
  };

  const refreshPricesMutation = useMutation({
    mutationFn: async () => {
      const pricesResult = await priceApi.refreshStockPrices(true);
      priceApi.refreshDividends().catch(console.error);
      return pricesResult;
    },
    onSuccess: (result) => {
      queryClient.invalidateQueries({ queryKey: ['investments'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['dividend-summary'] });
      if (result.rate_limit_hit) {
        toast(t('toast.rateLimitReached'), { description: t('toast.rateLimitDescription') });
      } else {
        toast(t('toast.pricesRefreshed', { count: result.updated.length }), {
          description: t('toast.pricesRefreshedDescription'),
        });
      }
    },
    onError: (error: Error) => {
      toast.error(t('toast.refreshFailed'), { description: error.message });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => investmentsApi.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['investments'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['all-stock-transactions'] });
      toast(t('toast.deleted'));
      setRowModal(null);
      setSelected(null);
    },
    onError: (error) => {
      toast.error(tc('status.error'), { description: String(error) });
    },
  });

  // Positions as the backend values them in the report currency (ADR 0011)
  const reportCurrency = positions?.currency ?? currencyCode;
  const holdings = useMemo(
    () =>
      (positions?.items ?? []).map(toStockRow).sort((a, b) =>
        (a.companyName || '').localeCompare(b.companyName || '', undefined, {
          sensitivity: 'base',
        })
      ),
    [positions]
  );
  const totals = positions?.totals;
  const totalCost = amountOf(totals?.costBasis) ?? 0;
  const unrealizedGain = amountOf(totals?.unrealizedGain) ?? 0;
  const top = topPerformer(holdings);
  const metrics = {
    totalValue: amountOf(totals?.value) ?? 0,
    totalCost,
    overallGainLoss: unrealizedGain,
    overallGainLossPercent: percentOfCost(unrealizedGain, totalCost),
    estimatedDividendYield: amountOf(totals?.yearlyDividend) ?? 0,
    topPerformer: top
      ? {
          ticker: top.ticker,
          companyName: top.companyName,
          gainLossPercent: top.gainLossPercent ?? 0,
        }
      : null,
  };
  const realizedGain = amountOf(totals?.realizedGain) ?? 0;
  const openPositions = holdings.filter((h) => h.quantity > 0);
  const latestFetchedAt = holdings.reduce<Date | undefined>((latest, h) => {
    if (!h.fetchedAt) return latest;
    const date = new Date(h.fetchedAt * 1000);
    return !latest || date > latest ? date : latest;
  }, undefined);

  const openRowModal = (modal: RowModal) => (holding: HoldingData) => {
    setSelected(holding);
    setRowModal(modal);
  };
  const closeRowModal = (open: boolean) => {
    if (!open) {
      setRowModal(null);
      setSelected(null);
    }
  };

  const isEmpty = !isLoading && holdings.length === 0;

  return (
    <>
      <PageHead
        eyebrow={t('eyebrow')}
        title={t('title')}
        description={t('subtitle')}
        actions={
          !isEmpty && (
            <>
              {/* Four actions: below 1280 px refresh and export show only their icons */}
              <Button
                variant="outline"
                onClick={() => refreshPricesMutation.mutate()}
                disabled={refreshPricesMutation.isPending}
                loading={refreshPricesMutation.isPending}
                title={t('refreshPrices')}
                className="max-xl:px-[11px]"
              >
                <RefreshCw />
                <span className="max-xl:sr-only">{t('refreshPrices')}</span>
              </Button>
              <ExportButton exportFn={exportApi.stockTransactions} label={t('export')} compact />
              <Button variant="outline" onClick={() => setImportOpen(true)}>
                <Upload />
                {t('importCSV')}
              </Button>
              <Button onClick={() => setAddOpen(true)}>
                <Plus />
                {t('addInvestment')}
              </Button>
            </>
          )
        }
      />

      {isLoading ? (
        <Stats className="grid-cols-[1.2fr_0.9fr_0.9fr_0.9fr]">
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
        </Stats>
      ) : isEmpty ? (
        <EmptyState
          icon={<TrendingUp />}
          title={t('empty.title')}
          description={t('empty.description')}
          action={
            <div className="flex flex-wrap items-center justify-center gap-2">
              <Button onClick={() => setAddOpen(true)}>
                <Plus />
                {t('addInvestment')}
              </Button>
              <Button variant="outline" onClick={() => setImportOpen(true)}>
                <Upload />
                {t('importCSV')}
              </Button>
            </div>
          }
        />
      ) : (
        <>
          <ConversionQualityNote quality={positions?.fxQuality} currency={reportCurrency} />
          <InvestmentsSummary
            currency={reportCurrency}
            metrics={metrics}
            positionCount={openPositions.length}
            realizedGain={realizedGain}
            isLoading={refreshPricesMutation.isPending}
            latestFetchedAt={latestFetchedAt}
          />

          <PortfolioTrendCard<TradeDayEvent>
            type="investments"
            currentValue={metrics.totalValue}
            isRefreshing={refreshPricesMutation.isPending}
            earliest={earliest}
            events={trendEvents}
            renderEventTip={trendEventTip}
            legend={
              <ChartLegend
                items={[
                  { label: t('chart.legend.value'), swatch: { kind: 'line' } },
                  { label: t('chart.legend.buy'), swatch: { kind: 'event', type: 'buy' } },
                  { label: t('chart.legend.sell'), swatch: { kind: 'event', type: 'sell' } },
                ]}
              />
            }
          />

          <InvestmentsTable
            currency={reportCurrency}
            holdings={holdings}
            tagsByInvestmentId={tagsByInvestmentId}
            isLoading={refreshPricesMutation.isPending}
            onViewDetail={(h) => setLocation(`/stocks/${h.id}`)}
            onBuy={openRowModal('buy')}
            onSell={openRowModal('sell')}
            onUpdatePrice={openRowModal('price')}
            onDelete={openRowModal('delete')}
          />
        </>
      )}

      <AddInvestmentModal open={addOpen} onOpenChange={setAddOpen} />
      {/* One instance for both layouts: after the import the table replaces the empty
          state, and the result screen must survive that switch */}
      <StockImportDialog open={importOpen} onOpenChange={setImportOpen} />
      <BuyInvestmentModal
        investment={selected}
        open={rowModal === 'buy'}
        onOpenChange={closeRowModal}
      />
      <SellInvestmentModal
        investment={selected}
        open={rowModal === 'sell'}
        onOpenChange={closeRowModal}
      />
      <ManualPriceModal
        investment={selected}
        open={rowModal === 'price'}
        onOpenChange={closeRowModal}
      />
      <ConfirmDeleteDialog
        open={rowModal === 'delete'}
        onOpenChange={closeRowModal}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.description', { name: selected?.companyName ?? '' })}
        onConfirm={() => selected && deleteMutation.mutate(selected.id)}
        isPending={deleteMutation.isPending}
        confirmLabel={t('actions.deletePosition')}
      />
    </>
  );
}
