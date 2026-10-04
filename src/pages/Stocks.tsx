import { useMemo, useState } from 'react';
import { useLocation } from 'wouter';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Plus, RefreshCw, TrendingUp, Upload } from 'lucide-react';
import type { StockInvestmentWithPrice } from '@shared/types';
import type { InvestmentTransaction } from '@shared/schema';
import type { CurrencyCode } from '@shared/currencies';
import { calculatePositionCostBasis, calculateRealizedGains } from '@shared/calculations';
import { exportApi, investmentsApi, priceApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useStockTagsByInvestment } from '@/hooks/use-stock-tags';
import { useDatedConvert } from '@/hooks/use-dated-convert';
import { mapInvestmentToHolding, calculateMetrics, type HoldingData } from '@/utils/stocks';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/common/EmptyState';
import { ExportButton } from '@/components/common/ExportButton';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { StatSkeleton, Stats } from '@/components/common/Stat';
import PortfolioTrendCard, { type TransactionMarker } from '@/components/common/PortfolioTrendCard';
import { AddInvestmentModal } from '@/components/stocks/AddInvestmentModal';
import { BuyInvestmentModal } from '@/components/stocks/BuyInvestmentModal';
import { SellInvestmentModal } from '@/components/stocks/SellInvestmentModal';
import { ManualPriceModal } from '@/components/stocks/ManualPriceModal';
import { ImportInvestmentsModal } from '@/components/stocks/ImportInvestmentsModal';
import { InvestmentsSummary } from '@/components/stocks/InvestmentsSummary';
import { InvestmentsTable } from '@/components/stocks/InvestmentsTable';

type RowModal = 'buy' | 'sell' | 'price' | 'delete' | null;

export default function Stocks() {
  const tagsByInvestmentId = useStockTagsByInvestment();
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const { convert, currencyCode } = useCurrency();
  const [addOpen, setAddOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [rowModal, setRowModal] = useState<RowModal>(null);
  const [selected, setSelected] = useState<HoldingData | null>(null);

  const { data: investments, isLoading } = useQuery<StockInvestmentWithPrice[]>({
    queryKey: ['investments'],
    queryFn: () => investmentsApi.getAll(),
    refetchOnMount: true,
    staleTime: 60 * 1000,
  });

  const { data: allTransactions } = useQuery<InvestmentTransaction[]>({
    queryKey: ['all-stock-transactions'],
    queryFn: () => investmentsApi.getAllTransactions(),
    staleTime: 60 * 1000,
  });

  // Date-aware conversion over the transactions' range: historical amounts
  // convert at their own day's rates (ADR 0001).
  const convertAt = useDatedConvert(allTransactions);

  const txsByInvestment = useMemo(() => {
    const map = new Map<string, InvestmentTransaction[]>();
    for (const tx of allTransactions || []) {
      const group = map.get(tx.investmentId) ?? [];
      group.push(tx);
      map.set(tx.investmentId, group);
    }
    return map;
  }, [allTransactions]);

  // Earliest transaction bounds the "Vše" period of the trend card
  const transactionMarkers = useMemo((): TransactionMarker[] => {
    if (!allTransactions || allTransactions.length === 0) return [];
    const earliest = Math.min(...allTransactions.map((tx) => tx.transactionDate));
    return [{ date: Math.floor(earliest / 86400) * 86400, buyAmount: 0, sellAmount: 0 }];
  }, [allTransactions]);

  // Realized gains (WAC) in total and per position, each leg at its day's rate
  const realizedGain = useMemo(() => {
    if (!allTransactions || allTransactions.length === 0) return 0;
    return calculateRealizedGains(allTransactions, convertAt, currencyCode as CurrencyCode);
  }, [allTransactions, convertAt, currencyCode]);

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

  // Holdings in the display currency: market value from the native price at
  // today's rate, cost basis from the transactions at their own day's rates.
  const holdings = useMemo(() => {
    return (investments || [])
      .map((inv) => {
        const holding = mapInvestmentToHolding(inv);
        const preferredCurrency = currencyCode as CurrencyCode;
        const marketPriceCurrency = (inv.currency || 'USD') as CurrencyCode;
        const avgCostCurrency = (inv.averagePriceCurrency || 'USD') as CurrencyCode;

        const originalCurrentPrice = parseFloat(String(inv.originalPrice)) || 0;
        const currentPriceConverted =
          originalCurrentPrice > 0
            ? convert(originalCurrentPrice, marketPriceCurrency, preferredCurrency)
            : convert(holding.currentPrice, 'CZK', preferredCurrency);
        const marketValue = holding.quantity * currentPriceConverted;

        const txs = txsByInvestment.get(inv.id) ?? [];
        const position = calculatePositionCostBasis(txs, convertAt, preferredCurrency);
        const gainLoss = marketValue - position.costBasis;
        const gainLossPercent =
          position.costBasis !== 0 ? (gainLoss / position.costBasis) * 100 : 0;
        const positionRealized =
          txs.length > 0 ? calculateRealizedGains(txs, convertAt, preferredCurrency) : 0;

        return {
          ...holding,
          avgCost: position.averageCost,
          avgCostCurrency: preferredCurrency,
          currentPrice: currentPriceConverted,
          currency: preferredCurrency,
          totalCost: position.costBasis,
          originalAvgCost: holding.avgCost,
          originalAvgCostCurrency: avgCostCurrency,
          originalCurrentPrice,
          originalCurrency: marketPriceCurrency,
          marketValue,
          gainLoss,
          gainLossPercent,
          realizedGain: positionRealized,
        } satisfies HoldingData;
      })
      .sort((a, b) =>
        (a.companyName || '').localeCompare(b.companyName || '', undefined, { sensitivity: 'base' })
      );
  }, [investments, txsByInvestment, convertAt, convert, currencyCode]);

  const totalDividendYield = holdings.reduce(
    (sum, h) => sum + (h.dividendYield || 0) * h.quantity,
    0
  );
  const metrics = calculateMetrics(holdings, totalDividendYield);
  const openPositions = holdings.filter((h) => h.quantity > 0);
  const latestFetchedAt = holdings.reduce<Date | undefined>((latest, h) => {
    if (!h.fetchedAt) return latest;
    const value = h.fetchedAt;
    const date = new Date(typeof value === 'number' && value < 100000000000 ? value * 1000 : value);
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
              <Button
                variant="outline"
                onClick={() => refreshPricesMutation.mutate()}
                disabled={refreshPricesMutation.isPending}
                loading={refreshPricesMutation.isPending}
              >
                <RefreshCw />
                {t('refreshPrices')}
              </Button>
              <ExportButton exportFn={exportApi.stockTransactions} label={t('export')} />
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
          <InvestmentsSummary
            metrics={metrics}
            positionCount={openPositions.length}
            realizedGain={realizedGain}
            isLoading={refreshPricesMutation.isPending}
            latestFetchedAt={latestFetchedAt}
          />

          <PortfolioTrendCard
            type="investments"
            currentValue={metrics.totalValue}
            isRefreshing={refreshPricesMutation.isPending}
            transactionMarkers={transactionMarkers}
          />

          <InvestmentsTable
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

      <AddInvestmentModal
        open={addOpen}
        onOpenChange={setAddOpen}
        onImportCsv={() => setImportOpen(true)}
      />
      {/* One instance for both layouts: after the import the table replaces the empty
          state, and the result screen must survive that switch */}
      <ImportInvestmentsModal open={importOpen} onOpenChange={setImportOpen} />
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
