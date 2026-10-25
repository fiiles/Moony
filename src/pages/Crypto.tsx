import { useMemo, useState } from 'react';
import { invalidateAfterPriceRefresh } from '@/lib/price-refresh';
import { useLocation } from 'wouter';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Bitcoin, Plus, RefreshCw } from 'lucide-react';
import type { CryptoPositions, CryptoTransaction } from '@shared/schema';
import { cryptoApi, exportApi, priceApi } from '@/lib/tauri-api';
import { amountOf, toCryptoRow, totalOf } from '@/utils/positions';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useTransactionFx } from '@/hooks/use-fx';
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
import { McpTip } from '@/components/common/McpTip';
import { ExportButton } from '@/components/common/ExportButton';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { StatSkeleton, Stats } from '@/components/common/Stat';
import PortfolioTrendCard from '@/components/common/PortfolioTrendCard';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { AddCryptoModal } from '@/components/crypto/AddCryptoModal';
import { BuyCryptoModal } from '@/components/crypto/BuyCryptoModal';
import { SellCryptoModal } from '@/components/crypto/SellCryptoModal';
import { UpdateCryptoPriceModal } from '@/components/crypto/UpdateCryptoPriceModal';
import { CryptoSymbolDialog } from '@/components/crypto/CryptoSymbolDialog';
import { UnpricedCoinsBanner } from '@/components/crypto/UnpricedCoinsBanner';
import { CryptoSummary } from '@/components/crypto/CryptoSummary';
import { CryptoTable, type CryptoTableRow } from '@/components/crypto/CryptoTable';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';

type RowModal = 'buy' | 'sell' | 'price' | 'symbol' | 'delete' | null;

export default function Crypto() {
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const { currencyCode, fxRevision } = useCurrency();
  const fmt = useFormat();
  const [addOpen, setAddOpen] = useState(false);
  const [rowModal, setRowModal] = useState<RowModal>(null);
  const [selected, setSelected] = useState<CryptoTableRow | null>(null);

  const { data: positions, isLoading } = useQuery<CryptoPositions>({
    queryKey: ['crypto', currencyCode, fxRevision],
    queryFn: () => cryptoApi.getAll(currencyCode),
    refetchOnMount: true,
    staleTime: 60 * 1000,
  });

  const { data: allTransactions } = useQuery<CryptoTransaction[]>({
    queryKey: ['all-crypto-transactions'],
    queryFn: () => cryptoApi.getAllTransactions(),
    staleTime: 60 * 1000,
  });

  // The trade markers of the trend card: every trade in the report currency at the rates
  // of its own day (common-day rule), like the history line they sit on.
  const { convertAt } = useTransactionFx(allTransactions);

  const txsByInvestment = useMemo(() => {
    const map = new Map<string, CryptoTransaction[]>();
    for (const tx of allTransactions || []) {
      const group = map.get(tx.investmentId) ?? [];
      group.push(tx);
      map.set(tx.investmentId, group);
    }
    return map;
  }, [allTransactions]);

  // No horizon of the trend card starts before the first transaction
  const earliest = useMemo(() => firstTradeDay(allTransactions ?? []), [allTransactions]);

  // Buy and sell days as events on the aggregate trend (prototype crypto.html):
  // one marker per day and type, listing the coins and the day's total.
  const trendEvents = useMemo(
    () =>
      tradeDayEvents(allTransactions ?? [], (amount, currency, date) =>
        convertAt(amount, currency, currencyCode, date)
      ),
    [allTransactions, convertAt, currencyCode]
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
      lines: [
        event.amount === null
          ? fmt.day(event.t)
          : `${fmt.day(event.t)} · ${fmt.money(event.amount, currencyCode, { decimals: 0 })}`,
      ],
    };
  };

  const sellCount = (allTransactions ?? []).filter((tx) => tx.type === 'sell').length;

  const refreshPricesMutation = useMutation({
    mutationFn: () => priceApi.refreshCryptoPrices(true),
    onSuccess: (result) => {
      void invalidateAfterPriceRefresh(queryClient);
      toast(t('toast.pricesRefreshed', { count: result.length }), {
        description: t('toast.pricesRefreshedDescription'),
      });
    },
    onError: (error: Error) => {
      toast.error(t('toast.refreshFailed'), { description: error.message });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => cryptoApi.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['crypto'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['all-crypto-transactions'] });
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
  const totals = positions?.totals;
  const totalValue = totalOf(totals?.value);
  const holdings = useMemo((): CryptoTableRow[] => {
    const openTotal = amountOf(positions?.totals.value) ?? 0;
    return (positions?.items ?? []).map((inv) => {
      const row = toCryptoRow(inv);
      const lastSell = (txsByInvestment.get(inv.id) ?? [])
        .filter((tx) => tx.type === 'sell')
        .reduce<number | null>(
          (max, tx) => (max === null || tx.transactionDate > max ? tx.transactionDate : max),
          null
        );
      return {
        ...row,
        share:
          openTotal > 0 && row.quantity > 0 && row.marketValue !== null
            ? row.marketValue / openTotal
            : 0,
        closedAt: row.quantity <= 0 ? lastSell : null,
      };
    });
  }, [positions, txsByInvestment]);

  const openPositions = holdings.filter((h) => h.quantity > 0);
  const totalCost = totalOf(totals?.costBasis);
  const unrealizedGain = totalOf(totals?.unrealizedGain);
  const metrics = {
    totalValue,
    totalCost,
    overallGainLoss: unrealizedGain,
    overallGainLossPercent: totals?.unrealizedGainPercent ?? 0,
  };
  const realizedGain = amountOf(totals?.realizedGain) ?? 0;
  const largestRow = openPositions.reduce<CryptoTableRow | null>(
    (max, h) =>
      h.marketValue !== null && (max === null || h.marketValue > (max.marketValue ?? 0)) ? h : max,
    null
  );
  const latestFetchedAt = holdings.reduce<Date | undefined>((latest, h) => {
    if (!h.fetchedAt) return latest;
    const date = new Date(h.fetchedAt * 1000);
    return !latest || date > latest ? date : latest;
  }, undefined);

  const openRowModal = (modal: RowModal) => (holding: CryptoTableRow) => {
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
              <Button
                variant="outline"
                onClick={() => refreshPricesMutation.mutate()}
                disabled={refreshPricesMutation.isPending}
                loading={refreshPricesMutation.isPending}
              >
                <RefreshCw />
                {t('refreshPrices')}
              </Button>
              <ExportButton exportFn={exportApi.cryptoTransactions} label={t('export')} />
              <Button onClick={() => setAddOpen(true)}>
                <Plus />
                {t('addCrypto')}
              </Button>
            </>
          )
        }
      />

      {isLoading ? (
        <Stats>
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
        </Stats>
      ) : isEmpty ? (
        <EmptyState
          icon={<Bitcoin />}
          title={t('empty.title')}
          description={t('empty.description')}
          action={
            <div className="flex flex-col items-center gap-4">
              <Button onClick={() => setAddOpen(true)}>
                <Plus />
                {t('addCrypto')}
              </Button>
              <McpTip domain="crypto" />
            </div>
          }
        />
      ) : (
        <>
          <ConversionQualityNote quality={positions?.fxQuality} currency={reportCurrency} />
          <CryptoSummary
            currency={reportCurrency}
            metrics={metrics}
            positionCount={openPositions.length}
            realizedGain={realizedGain}
            sellCount={sellCount}
            largest={
              largestRow
                ? {
                    name: largestRow.name,
                    share: largestRow.share,
                    gainLossPercent: largestRow.gainLossPercent ?? 0,
                  }
                : null
            }
            latestFetchedAt={latestFetchedAt}
            isLoading={refreshPricesMutation.isPending}
          />

          <UnpricedCoinsBanner rows={holdings} onPick={openRowModal('symbol')} />

          <PortfolioTrendCard<TradeDayEvent>
            type="crypto"
            currentValue={metrics.totalValue}
            missingCurrencies={positions?.fxQuality.missingCurrencies}
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

          <CryptoTable
            currency={reportCurrency}
            totalValue={totalValue}
            holdings={holdings}
            isLoading={refreshPricesMutation.isPending}
            onViewDetail={(h) => setLocation(`/crypto/${h.id}`)}
            onBuy={openRowModal('buy')}
            onSell={openRowModal('sell')}
            onUpdatePrice={openRowModal('price')}
            onChangeSymbol={openRowModal('symbol')}
            onDelete={openRowModal('delete')}
          />
        </>
      )}

      <AddCryptoModal open={addOpen} onOpenChange={setAddOpen} />
      <BuyCryptoModal
        investment={selected}
        open={rowModal === 'buy'}
        onOpenChange={closeRowModal}
      />
      <SellCryptoModal
        investment={selected}
        open={rowModal === 'sell'}
        onOpenChange={closeRowModal}
      />
      <UpdateCryptoPriceModal
        investment={selected}
        open={rowModal === 'price'}
        onOpenChange={closeRowModal}
      />
      <CryptoSymbolDialog
        investment={selected}
        open={rowModal === 'symbol'}
        onOpenChange={closeRowModal}
      />
      <ConfirmDeleteDialog
        open={rowModal === 'delete'}
        onOpenChange={closeRowModal}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.description', {
          name: selected?.name ?? '',
          ticker: selected?.ticker ?? '',
        })}
        onConfirm={() => selected && deleteMutation.mutate(selected.id)}
        isPending={deleteMutation.isPending}
        confirmLabel={t('actions.delete')}
      />
    </>
  );
}
