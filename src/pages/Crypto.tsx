import { useMemo, useState } from 'react';
import { useLocation } from 'wouter';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Bitcoin, Plus, RefreshCw } from 'lucide-react';
import type { CryptoInvestmentWithPrice } from '@shared/types';
import type { CryptoTransaction } from '@shared/schema';
import type { CurrencyCode } from '@shared/currencies';
import {
  calculateCryptoPortfolioMetrics,
  calculatePositionCostBasis,
  calculateRealizedGains,
  mapCryptoInvestmentToHolding,
} from '@shared/calculations';
import { cryptoApi, exportApi, priceApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useDatedConvert } from '@/hooks/use-dated-convert';
import type { EventCluster } from '@/utils/chart-scale';
import { firstTradeDay, tradeDayEvents, type TradeDayEvent } from '@/utils/trade-events';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/common/EmptyState';
import { ExportButton } from '@/components/common/ExportButton';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { StatSkeleton, Stats } from '@/components/common/Stat';
import PortfolioTrendCard from '@/components/common/PortfolioTrendCard';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { AddCryptoModal } from '@/components/crypto/AddCryptoModal';
import { BuyCryptoModal } from '@/components/crypto/BuyCryptoModal';
import { SellCryptoModal } from '@/components/crypto/SellCryptoModal';
import { UpdateCryptoPriceModal } from '@/components/crypto/UpdateCryptoPriceModal';
import { CoinGeckoApiKeyBanner } from '@/components/crypto/CoinGeckoApiKeyBanner';
import { CryptoSummary } from '@/components/crypto/CryptoSummary';
import { CryptoTable, type CryptoRow } from '@/components/crypto/CryptoTable';

type RowModal = 'buy' | 'sell' | 'price' | 'delete' | null;

export default function Crypto() {
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const { convert, currencyCode, formatCurrency } = useCurrency();
  const fmt = useFormat();
  const [addOpen, setAddOpen] = useState(false);
  const [rowModal, setRowModal] = useState<RowModal>(null);
  const [selected, setSelected] = useState<CryptoRow | null>(null);

  const { data: cryptoInvestments, isLoading } = useQuery<CryptoInvestmentWithPrice[]>({
    queryKey: ['crypto'],
    queryFn: () => cryptoApi.getAll(),
    refetchOnMount: true,
    staleTime: 60 * 1000,
  });

  const { data: allTransactions } = useQuery<CryptoTransaction[]>({
    queryKey: ['all-crypto-transactions'],
    queryFn: () => cryptoApi.getAllTransactions(),
    staleTime: 60 * 1000,
  });

  // Date-aware conversion over the transactions' range: historical amounts
  // convert at their own day's rates (ADR 0001).
  const convertAt = useDatedConvert(allTransactions);
  const preferredCurrency = currencyCode as CurrencyCode;

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
        tickers: event.tickers.join(', '),
      }),
      lines: [`${fmt.day(event.t)} · ${formatCurrency(event.amountCzk)}`],
    };
  };

  // Realized gains (WAC) in total, each leg at its day's rate
  const realizedGain = useMemo(() => {
    if (!allTransactions || allTransactions.length === 0) return 0;
    return calculateRealizedGains(allTransactions, convertAt, preferredCurrency);
  }, [allTransactions, convertAt, preferredCurrency]);
  const sellCount = (allTransactions ?? []).filter((tx) => tx.type === 'sell').length;

  const refreshPricesMutation = useMutation({
    mutationFn: () => priceApi.refreshCryptoPrices(),
    onSuccess: (result) => {
      queryClient.invalidateQueries({ queryKey: ['crypto'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
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

  // Holdings in the display currency: market value from the native rate at
  // today's FX, cost basis from the transactions at their own day's rates.
  const holdings = useMemo((): CryptoRow[] => {
    const rows = (cryptoInvestments || []).map((inv) => {
      const quantity = parseFloat(String(inv.quantity)) || 0;
      const txs = txsByInvestment.get(inv.id) ?? [];
      const averagePrice = calculatePositionCostBasis(
        txs,
        convertAt,
        preferredCurrency
      ).averageCost;
      const originalPrice = parseFloat(String(inv.originalPrice)) || 0;
      const nativeCurrency = (inv.currency || 'USD') as CurrencyCode;
      const currentPrice =
        originalPrice > 0
          ? convert(originalPrice, nativeCurrency, preferredCurrency)
          : convert(parseFloat(String(inv.currentPrice)) || 0, 'CZK', preferredCurrency);
      const holding = mapCryptoInvestmentToHolding(
        inv.id,
        inv.ticker,
        inv.name || inv.ticker,
        quantity,
        averagePrice,
        currentPrice,
        inv.fetchedAt,
        inv.isManualPrice,
        inv.coingeckoId,
        inv.originalPrice,
        inv.currency
      );
      const lastSell = txs
        .filter((tx) => tx.type === 'sell')
        .reduce<number | null>(
          (max, tx) => (max === null || tx.transactionDate > max ? tx.transactionDate : max),
          null
        );
      return {
        ...holding,
        realizedGain:
          txs.length > 0 ? calculateRealizedGains(txs, convertAt, preferredCurrency) : 0,
        share: 0,
        closedAt: quantity <= 0 ? lastSell : null,
      };
    });
    const openTotal = rows.reduce((sum, h) => sum + (h.quantity > 0 ? h.marketValue : 0), 0);
    return rows.map((h) => ({
      ...h,
      share: openTotal > 0 && h.quantity > 0 ? h.marketValue / openTotal : 0,
    }));
  }, [cryptoInvestments, txsByInvestment, convertAt, convert, preferredCurrency]);

  const openPositions = holdings.filter((h) => h.quantity > 0);
  const metrics = calculateCryptoPortfolioMetrics(openPositions);
  const largestRow = openPositions.reduce<CryptoRow | null>(
    (max, h) => (max === null || h.marketValue > max.marketValue ? h : max),
    null
  );
  const latestFetchedAt = holdings.reduce<Date | undefined>((latest, h) => {
    if (!h.fetchedAt) return latest;
    const value = h.fetchedAt as unknown;
    const date = new Date(
      typeof value === 'number' && value < 100000000000 ? value * 1000 : (value as Date)
    );
    return !latest || date > latest ? date : latest;
  }, undefined);

  const openRowModal = (modal: RowModal) => (holding: CryptoRow) => {
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
            <Button onClick={() => setAddOpen(true)}>
              <Plus />
              {t('addCrypto')}
            </Button>
          }
        />
      ) : (
        <>
          <CryptoSummary
            metrics={metrics}
            positionCount={openPositions.length}
            realizedGain={realizedGain}
            sellCount={sellCount}
            largest={
              largestRow
                ? {
                    name: largestRow.name,
                    share: largestRow.share,
                    gainLossPercent: largestRow.gainLossPercent,
                  }
                : null
            }
            latestFetchedAt={latestFetchedAt}
            isLoading={refreshPricesMutation.isPending}
          />

          <CoinGeckoApiKeyBanner />

          <PortfolioTrendCard<TradeDayEvent>
            type="crypto"
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

          <CryptoTable
            holdings={holdings}
            isLoading={refreshPricesMutation.isPending}
            onViewDetail={(h) => setLocation(`/crypto/${h.id}`)}
            onBuy={openRowModal('buy')}
            onSell={openRowModal('sell')}
            onUpdatePrice={openRowModal('price')}
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
