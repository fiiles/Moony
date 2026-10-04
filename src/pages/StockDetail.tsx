import { Fragment, useMemo, useState } from 'react';
import { useLocation, useRoute } from 'wouter';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import {
  ChartCandlestick,
  Ellipsis,
  Info,
  Pencil,
  Plus,
  Receipt,
  RefreshCw,
  Tag,
  Trash2,
} from 'lucide-react';
import type { InvestmentTransaction, StockCompanyInfo } from '@shared/schema';
import type { StockInvestmentWithPrice } from '@shared/types/extended-types';
import { type CurrencyCode, priceDecimals } from '@shared/currencies';
import { calculatePositionCostBasis, calculateRealizedGains } from '@shared/calculations';
import { investmentsApi, priceApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useDatedConvert } from '@/hooks/use-dated-convert';
import { useInvestmentTransactionMutations } from '@/hooks/use-investment-mutations';
import { useShellPage } from '@/components/shell/shell-context';
import { formatAmountWithCode } from '@/utils/format-amount';
import { mapInvestmentToHolding } from '@/utils/stocks';
import { utcDayFloor } from '@/utils/chart-axis';
import { formatMarketCap } from '@/utils/stock-monitor';
import { BackLink, PageHead } from '@/components/shell/PageHead';
import { HeroValue } from '@/components/common/HeroCard';
import { Stat, Stats } from '@/components/common/Stat';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Segmented } from '@/components/ui/segmented';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { BuyInvestmentModal } from '@/components/stocks/BuyInvestmentModal';
import { SellInvestmentModal } from '@/components/stocks/SellInvestmentModal';
import { ManualPriceModal } from '@/components/stocks/ManualPriceModal';
import { ManualDividendModal } from '@/components/stocks/ManualDividendModal';
import { EditTransactionModal } from '@/components/stocks/EditTransactionModal';
import { PositionHero } from '@/components/stocks/PositionHero';

type LedgerFilter = 'all' | 'buy' | 'sell';

/** Company data moves slowly and the backend asks Yahoo at most once a day, so within the hour
 *  a repeat visit reuses what the card already has. */
const COMPANY_INFO_STALE_TIME_MS = 60 * 60 * 1000;

export default function StockDetail() {
  const [, params] = useRoute('/stocks/:id');
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const id = params?.id;
  const { formatCurrencyRaw, currencyCode, convert } = useCurrency();
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();

  const [showBuyModal, setShowBuyModal] = useState(false);
  const [showSellModal, setShowSellModal] = useState(false);
  const [showPriceModal, setShowPriceModal] = useState(false);
  const [showDividendModal, setShowDividendModal] = useState(false);
  const [showEditNameModal, setShowEditNameModal] = useState(false);
  const [showDeleteDialog, setShowDeleteDialog] = useState(false);
  const [editableName, setEditableName] = useState('');
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [editingTransaction, setEditingTransaction] = useState<InvestmentTransaction | null>(null);
  const [pendingDelete, setPendingDelete] = useState<InvestmentTransaction | null>(null);
  const [filter, setFilter] = useState<LedgerFilter>('all');
  const [hotEventId, setHotEventId] = useState<string | null>(null);
  const { deleteTransaction } = useInvestmentTransactionMutations();

  const { data: investment, isLoading } = useQuery<StockInvestmentWithPrice | null>({
    queryKey: ['investment', id],
    queryFn: () => investmentsApi.get(id!),
    enabled: !!id,
  });

  const { data: transactions } = useQuery<InvestmentTransaction[]>({
    queryKey: ['investment-transactions', id],
    queryFn: () => investmentsApi.getTransactions(id!),
    enabled: !!id,
  });

  // Company data comes from Yahoo through the stock_data cache. The stored record shows at once;
  // the second query lets the backend fetch it first when it is missing or over a day old, so a
  // slow Yahoo answer never holds back what is already known.
  const ticker = investment?.ticker;
  const storedCompanyInfo = useQuery<StockCompanyInfo>({
    queryKey: ['stock-company-info', ticker, 'stored'],
    queryFn: () => investmentsApi.getCompanyInfo(ticker!, false),
    enabled: !!ticker,
    staleTime: COMPANY_INFO_STALE_TIME_MS,
  });
  const freshCompanyInfo = useQuery<StockCompanyInfo>({
    queryKey: ['stock-company-info', ticker, 'fresh'],
    queryFn: () => investmentsApi.getCompanyInfo(ticker!, true),
    enabled: !!ticker,
    staleTime: COMPANY_INFO_STALE_TIME_MS,
  });
  const companyInfo = freshCompanyInfo.data ?? storedCompanyInfo.data;
  const companyFetching = freshCompanyInfo.isFetching;
  const refetchCompanyInfo = freshCompanyInfo.refetch;

  // Date-aware conversion over the transactions' range (ADR 0001).
  const convertAt = useDatedConvert(transactions);
  const preferredCurrency = currencyCode as CurrencyCode;

  const deleteMutation = useMutation({
    mutationFn: () => investmentsApi.delete(id!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['investments'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      toast(t('toast.deleted'));
      setLocation('/stocks');
    },
    onError: (error) => {
      toast.error(tc('status.error'), { description: String(error) });
    },
  });

  const updateNameMutation = useMutation({
    mutationFn: (newName: string) => investmentsApi.updateName(id!, newName),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['investment', id] });
      queryClient.invalidateQueries({ queryKey: ['investments'] });
      setShowEditNameModal(false);
      toast(t('detail.nameUpdated'));
    },
    onError: (error) => {
      toast.error(tc('status.error'), { description: String(error) });
    },
  });

  const handleRefreshPrices = async () => {
    setIsRefreshing(true);
    try {
      await priceApi.refreshStockPrices();
      await queryClient.invalidateQueries({ queryKey: ['investment', id] });
      await queryClient.invalidateQueries({ queryKey: ['investments'] });
      // A first price creates the stock_data row the company data is stored in. Not awaited: a
      // Yahoo round trip must not keep the refresh button spinning.
      void queryClient.invalidateQueries({ queryKey: ['stock-company-info'] });
      toast(t('detail.pricesRefreshed'));
    } catch (error) {
      toast.error(tc('status.error'), { description: String(error) });
    } finally {
      setIsRefreshing(false);
    }
  };

  // Position numbers (display currency); cost from the transactions at their own day's rates
  const holdingData = investment ? mapInvestmentToHolding(investment) : null;
  const quantity = holdingData?.quantity ?? 0;
  const position = useMemo(
    () => calculatePositionCostBasis(transactions ?? [], convertAt, preferredCurrency),
    [transactions, convertAt, preferredCurrency]
  );
  const realized = useMemo(
    () =>
      transactions && transactions.length > 0
        ? calculateRealizedGains(transactions, convertAt, preferredCurrency)
        : 0,
    [transactions, convertAt, preferredCurrency]
  );
  const stockCurrency = (investment?.currency || 'USD') as CurrencyCode;
  const avgCostCurrency = (investment?.averagePriceCurrency || 'USD') as CurrencyCode;
  const originalPriceNum = parseFloat(String(investment?.originalPrice)) || 0;
  const currentPrice =
    originalPriceNum > 0
      ? convert(originalPriceNum, stockCurrency, preferredCurrency)
      : convert(holdingData?.currentPrice ?? 0, 'CZK', preferredCurrency);
  const currentValue = quantity * currentPrice;
  const totalInvested = position.costBasis;
  const unrealized = currentValue - totalInvested;
  const unrealizedPct = totalInvested > 0 ? unrealized / totalInvested : 0;
  const dividendPerShare = investment?.dividendYield || 0;
  const yearlyDividend = quantity * dividendPerShare;
  const dividendYieldPct = currentValue > 0 ? yearlyDividend / currentValue : 0;

  const sells = (transactions ?? []).filter((tx) => tx.type === 'sell');
  const lastBuy = (transactions ?? [])
    .filter((tx) => tx.type === 'buy')
    .reduce<number | null>(
      (max, tx) => (max === null || tx.transactionDate > max ? tx.transactionDate : max),
      null
    );

  // Top bar: entity name and freshness of the price
  const fetchedAt = investment?.fetchedAt ? Number(investment.fetchedAt) : null;
  const fetchedToday =
    fetchedAt !== null && utcDayFloor(fetchedAt) === utcDayFloor(Date.now() / 1000);
  useShellPage({
    crumb: investment?.companyName,
    status:
      fetchedAt !== null
        ? {
            text: t('detail.status.pricesUpdated', {
              when: fetchedToday
                ? t('detail.status.todayAt', { time: fmt.time(fetchedAt) })
                : fmt.dateTime(fetchedAt),
            }),
            tone: fetchedToday ? 'fresh' : 'neutral',
          }
        : undefined,
  });

  const ledger = useMemo(() => {
    const rows = [...(transactions ?? [])].sort((a, b) => b.transactionDate - a.transactionDate);
    return filter === 'all' ? rows : rows.filter((tx) => tx.type === filter);
  }, [transactions, filter]);
  const hasMixedCurrencies = (transactions ?? []).some(
    (tx) => tx.currency && tx.currency !== currencyCode
  );
  const nativeCurrencies = Array.from(
    new Set((transactions ?? []).map((tx) => tx.currency || currencyCode))
  );
  const firstTx = (transactions ?? []).reduce<number | null>(
    (min, tx) => (min === null || tx.transactionDate < min ? tx.transactionDate : min),
    null
  );

  const scrollToRow = (ids: string[]) => {
    const el = document.getElementById(`tx-${ids[0]}`);
    el?.scrollIntoView({ behavior: 'smooth', block: 'center' });
  };

  const pendingDeleteDescription = pendingDelete
    ? t('detail.confirmDeleteTransaction.description', {
        type: (pendingDelete.type === 'buy' ? tc('buttons.buy') : tc('buttons.sell')).toLowerCase(),
        quantity: fmt.number(Number(pendingDelete.quantity), { maximumFractionDigits: 4 }),
        ticker: pendingDelete.ticker,
        date: fmt.day(pendingDelete.transactionDate),
        amount: formatAmountWithCode(
          (parseFloat(pendingDelete.quantity) || 0) * (parseFloat(pendingDelete.pricePerUnit) || 0),
          pendingDelete.currency,
          fmt.locale
        ),
      })
    : '';

  if (isLoading) {
    return (
      <>
        <BackLink href="/stocks">{t('detail.backToList')}</BackLink>
        <div className="mb-[26px]">
          <div className="skeleton h-2.5 w-32" />
          <div className="skeleton mt-3 h-9 w-72" />
          <div className="skeleton mt-3 h-3 w-96" />
        </div>
        <Card className="px-6 pb-4 pt-6">
          <div className="skeleton h-3 w-24" />
          <div className="skeleton mt-3 h-10 w-56" />
          <div className="skeleton mt-6 h-[230px] w-full" />
        </Card>
      </>
    );
  }

  if (!investment || !holdingData) {
    return (
      <>
        <BackLink href="/stocks">{t('detail.backToList')}</BackLink>
        <p className="mt-8 text-center text-ink-3">{t('detail.notFound')}</p>
      </>
    );
  }

  const kv = (label: string, value: string | null | undefined) => (
    <div className="grid grid-cols-[1fr_auto] gap-x-4 gap-y-2.5 border-b border-line-soft py-[9px] text-table text-ink-3 last:border-0">
      <span>{label}</span>
      <b className="font-600 text-ink num">{value || '—'}</b>
    </div>
  );

  // "About the company": only what Yahoo reported. The stored figures are in the listing currency,
  // which differs from the active price's currency when that price was set by hand.
  const companyCurrency = companyInfo?.currency ?? stockCurrency;
  const figureText = (raw: string | null | undefined, format: (n: number) => string) => {
    const n = raw ? Number(raw) : NaN;
    return Number.isFinite(n) ? format(n) : null;
  };
  const ratioText = (n: number) => fmt.number(n, { maximumFractionDigits: 1 });
  const priceText = (n: number) =>
    fmt.money(n, companyCurrency, { decimals: priceDecimals(n, companyCurrency) });
  const companyProfile = [companyInfo?.sector, companyInfo?.industry].filter(Boolean).join(' · ');
  const companyRows = (
    companyInfo
      ? [
          [t('detail.companyInfo.peRatio'), figureText(companyInfo.peRatio, ratioText)],
          [t('detail.companyInfo.forwardPe'), figureText(companyInfo.forwardPe, ratioText)],
          [
            t('detail.companyInfo.marketCap'),
            formatMarketCap(companyInfo.marketCap, companyCurrency, fmt.locale),
          ],
          [
            t('detail.companyInfo.beta'),
            figureText(companyInfo.beta, (n) => fmt.number(n, { maximumFractionDigits: 2 })),
          ],
          [
            t('detail.companyInfo.fiftyTwoWeekHigh'),
            figureText(companyInfo.fiftyTwoWeekHigh, priceText),
          ],
          [
            t('detail.companyInfo.fiftyTwoWeekLow'),
            figureText(companyInfo.fiftyTwoWeekLow, priceText),
          ],
          [t('detail.companyInfo.dividendRate'), figureText(companyInfo.dividendRate, priceText)],
          [
            t('detail.companyInfo.dividendYield'),
            // A raw fraction: 0.0331 is 3.31 %
            figureText(companyInfo.dividendYield, (n) => fmt.percent(n, 2)),
          ],
        ]
      : []
  ).filter((row): row is [string, string] => row[1] !== null);

  const hasCompanyData = companyProfile !== '' || companyRows.length > 0;
  // Nothing to show yet and the refresh has not answered: placeholder rows, not "unavailable"
  const companyPending = !hasCompanyData && (freshCompanyInfo.isPending || companyFetching);

  const renderCompanyInfo = () => {
    if (hasCompanyData) {
      return (
        <>
          {companyProfile && (
            // Stacked: sector and industry names are long, the aside card is narrow
            <div className="border-b border-line-soft py-[9px] text-table text-ink-3">
              <span>{t('detail.companyInfo.sectorIndustry')}</span>
              <b className="mt-1 block font-600 text-ink">{companyProfile}</b>
            </div>
          )}
          {companyRows.map(([label, value]) => (
            <Fragment key={label}>{kv(label, value)}</Fragment>
          ))}
        </>
      );
    }
    if (companyPending) {
      return [0, 1, 2, 3, 4].map((row) => (
        <div
          key={row}
          aria-hidden
          className="flex items-center justify-between border-b border-line-soft py-[13px]"
        >
          <div className="skeleton h-3 w-2/5" />
          <div className="skeleton h-3 w-1/5" />
        </div>
      ));
    }
    if (companyInfo?.quoteType === 'ETF') {
      return (
        <p className="m-0 py-[9px] text-table leading-[1.5] text-ink-3">
          {t('detail.companyInfo.etfNote')}
        </p>
      );
    }
    return (
      <div className="py-[9px]">
        <p className="m-0 text-table leading-[1.5] text-ink-3">
          {t('detail.companyInfo.unavailable')}
        </p>
        <Button
          variant="outline"
          size="sm"
          className="mt-2.5"
          onClick={() => refetchCompanyInfo()}
          loading={companyFetching}
        >
          <RefreshCw />
          {t('detail.companyInfo.retry')}
        </Button>
      </div>
    );
  };

  return (
    <>
      <BackLink href="/stocks">{t('detail.backToList')}</BackLink>
      <PageHead
        eyebrow={[investment.ticker, stockCurrency]}
        title={investment.companyName}
        description={[
          t('detail.sharesCount', { count: quantity }),
          t('detail.avgBuyPrice', {
            price: fmt.money(position.averageCost, currencyCode, {
              decimals: priceDecimals(position.averageCost, currencyCode),
            }),
          }),
          lastBuy !== null ? t('detail.lastBuy', { date: fmt.day(lastBuy) }) : null,
        ]
          .filter(Boolean)
          .join(' · ')}
        actions={
          <>
            <Button
              variant="outline"
              onClick={handleRefreshPrices}
              disabled={isRefreshing}
              loading={isRefreshing}
            >
              <RefreshCw />
              {t('detail.refreshPrices')}
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="icon" aria-label={tc('labels.moreActions')}>
                  <Ellipsis />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onSelect={() => setShowPriceModal(true)}>
                  <Pencil />
                  {t('detail.menu.editPrice')}
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setShowDividendModal(true)}>
                  <Receipt />
                  {t('detail.menu.editDividend')}
                </DropdownMenuItem>
                <DropdownMenuItem
                  onSelect={() => {
                    setEditableName(investment.companyName);
                    setShowEditNameModal(true);
                  }}
                >
                  <Tag />
                  {t('detail.menu.editName')}
                </DropdownMenuItem>
                <DropdownMenuItem
                  onSelect={() =>
                    setLocation(`/stock-monitor/${encodeURIComponent(investment.ticker)}`)
                  }
                  data-testid="open-in-stock-monitor"
                >
                  <ChartCandlestick />
                  {t('detail.openInMonitor')}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="danger" onSelect={() => setShowDeleteDialog(true)}>
                  <Trash2 />
                  {t('actions.deletePosition')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <Button
              variant="outline"
              onClick={() => setShowSellModal(true)}
              disabled={quantity <= 0}
            >
              {t('detail.sell')}
            </Button>
            <Button onClick={() => setShowBuyModal(true)}>
              <Plus />
              {t('detail.buy')}
            </Button>
          </>
        }
      />

      <PositionHero
        type="stock"
        ticker={investment.ticker}
        currentValue={currentValue}
        currentCost={totalInvested}
        transactions={transactions ?? []}
        convertAt={convertAt}
        hotEventId={hotEventId}
        onHotEventChange={setHotEventId}
        onEventClick={scrollToRow}
        isRefreshing={isRefreshing}
        unit={t('table.unit')}
      >
        <HeroValue
          label={t('detail.positionValue')}
          value={formatCurrencyRaw(currentValue)}
          tone={unrealized > 0 ? 'gain' : unrealized < 0 ? 'loss' : 'neutral'}
          delta={
            totalInvested > 0
              ? `${unrealized >= 0 ? '↗' : '↘'} ${fmt.money(unrealized, currencyCode, { signed: true, decimals: 0 })}`
              : undefined
          }
          deltaNote={
            totalInvested > 0
              ? `${fmt.percent(unrealizedPct, 1, { signed: true })} ${t('detail.sinceBuy')}`
              : undefined
          }
        />
        <HeroValue
          compact
          label={t('detail.currentPrice')}
          value={fmt.money(currentPrice, currencyCode, {
            decimals: priceDecimals(currentPrice, currencyCode),
          })}
          delta={
            investment.isManualPrice ? (
              <Badge variant="outline">{t('badges.manual')}</Badge>
            ) : undefined
          }
          deltaNote={
            originalPriceNum > 0 && stockCurrency !== currencyCode
              ? fmt.money(originalPriceNum, stockCurrency, {
                  decimals: priceDecimals(originalPriceNum, stockCurrency),
                })
              : undefined
          }
        />
      </PositionHero>

      <Stats className="mt-[17px]">
        <Stat
          label={t('detail.invested')}
          value={formatCurrencyRaw(totalInvested)}
          note={`${t('detail.sharesCount', { count: quantity })} · ${t('detail.avgShort', {
            price: fmt.money(position.averageCost, currencyCode, {
              decimals: priceDecimals(position.averageCost, currencyCode),
            }),
          })}`}
        />
        <Stat
          label={t('detail.unrealized')}
          value={fmt.money(unrealized, currencyCode, { signed: true, decimals: 0 })}
          tone={unrealized > 0 ? 'gain' : unrealized < 0 ? 'loss' : 'neutral'}
          note={`${fmt.percent(unrealizedPct, 1, { signed: true })} ${t('detail.vsInvested')}`}
        />
        <Stat
          label={t('detail.realized')}
          value={fmt.money(realized, currencyCode, { signed: true, decimals: 0 })}
          tone={realized > 0 ? 'gain' : realized < 0 ? 'loss' : 'neutral'}
          noteTone="neutral"
          note={sells.length > 0 ? t('detail.sells', { count: sells.length }) : t('detail.noSells')}
        />
        <Stat
          label={t('detail.dividends')}
          aside={
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  className="rounded-full text-ink-4 hover:text-ink-2 focus-visible:outline-none focus-visible:shadow-focus"
                  aria-label={t('detail.dividendYieldTooltip')}
                >
                  <Info className="size-3.5" />
                </button>
              </TooltipTrigger>
              <TooltipContent>{t('detail.dividendYieldTooltip')}</TooltipContent>
            </Tooltip>
          }
          value={yearlyDividend > 0 ? formatCurrencyRaw(yearlyDividend) : '—'}
          note={
            yearlyDividend > 0
              ? `${t('detail.dividendNote', { percent: fmt.percent(dividendYieldPct, 1) })}${investment.isManualDividend ? ` · ${t('detail.manual')}` : ''}`
              : t('detail.noDividend')
          }
        />
      </Stats>

      <div className="grid grid-cols-[1.4fr_0.6fr] items-start gap-[18px]">
        <Card variant="table">
          <CardHeader>
            <div>
              <CardTitle>{t('detail.transactions.title')}</CardTitle>
              <CardDescription>
                {t('detail.ledger.sub', { count: transactions?.length ?? 0 })}
              </CardDescription>
            </div>
            <Segmented
              value={filter}
              onValueChange={setFilter}
              options={[
                { value: 'all', label: t('detail.ledger.filter.all') },
                { value: 'buy', label: t('detail.ledger.filter.buys') },
                { value: 'sell', label: t('detail.ledger.filter.sells') },
              ]}
            />
          </CardHeader>
          {ledger.length === 0 ? (
            <p className="border-t border-line px-[14px] py-10 text-center text-table text-ink-3">
              {t('detail.transactions.empty')}
            </p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('detail.transactions.date')}</TableHead>
                  <TableHead>{t('detail.transactions.type')}</TableHead>
                  <TableHead className="text-right">{t('detail.transactions.quantity')}</TableHead>
                  {hasMixedCurrencies && (
                    <TableHead className="text-right">
                      {nativeCurrencies.length === 1
                        ? t('detail.priceNative', { currency: nativeCurrencies[0] })
                        : t('detail.priceNativeMixed')}
                    </TableHead>
                  )}
                  <TableHead className="text-right">
                    {hasMixedCurrencies
                      ? t('detail.priceNative', { currency: currencyCode })
                      : t('detail.transactions.price')}
                  </TableHead>
                  <TableHead className="text-right">{t('detail.transactions.total')}</TableHead>
                  <TableHead className="w-16">
                    <span className="sr-only">{t('table.actions')}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {ledger.map((tx) => {
                  const txQty = parseFloat(tx.quantity) || 0;
                  const txPrice = parseFloat(tx.pricePerUnit) || 0;
                  const txCurrency = (tx.currency || currencyCode) as CurrencyCode;
                  const priceInDisplay = convertAt(
                    txPrice,
                    txCurrency,
                    preferredCurrency,
                    tx.transactionDate
                  );
                  const isSell = tx.type === 'sell';
                  return (
                    <TableRow
                      key={tx.id}
                      id={`tx-${tx.id}`}
                      className="h-[50px]"
                      data-hot={hotEventId === tx.id || undefined}
                      onMouseEnter={() => setHotEventId(tx.id)}
                      onMouseLeave={() => setHotEventId(null)}
                    >
                      <TableCell>{fmt.day(tx.transactionDate)}</TableCell>
                      <TableCell>
                        <Badge variant={isSell ? 'outline' : 'dark'}>
                          {isSell ? tc('buttons.sell') : tc('buttons.buy')}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-right num">
                        {fmt.number(txQty, { maximumFractionDigits: 4 })} {t('table.unit')}
                      </TableCell>
                      {hasMixedCurrencies && (
                        <TableCell className="text-right num">
                          {fmt.money(txPrice, txCurrency, {
                            decimals: priceDecimals(txPrice, txCurrency),
                          })}
                        </TableCell>
                      )}
                      <TableCell className="text-right num">
                        {fmt.money(priceInDisplay, currencyCode, {
                          decimals: priceDecimals(priceInDisplay, currencyCode),
                        })}
                      </TableCell>
                      <TableCell className="text-right font-650 text-ink num">
                        {formatCurrencyRaw(txQty * priceInDisplay)}
                      </TableCell>
                      <TableCell className="text-right">
                        <div data-row-actions className="inline-flex gap-0.5">
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            onClick={() => setEditingTransaction(tx)}
                            aria-label={tc('buttons.edit')}
                            title={tc('buttons.edit')}
                          >
                            <Pencil />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            onClick={() => setPendingDelete(tx)}
                            aria-label={tc('buttons.delete')}
                            title={tc('buttons.delete')}
                          >
                            <Trash2 />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          )}
          <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
            <span>
              {t('detail.ledger.footer', { count: ledger.length })}
              {firstTx !== null && ` · ${t('detail.ledger.since', { date: fmt.day(firstTx) })}`}
            </span>
            <span>{t('detail.ledger.rates', { currency: currencyCode })}</span>
          </div>
        </Card>

        <aside className="grid gap-[14px]">
          <Card>
            <CardHeader className="pb-1.5">
              <CardTitle className="text-h3">{t('detail.about')}</CardTitle>
              <Badge variant="outline">{t('detail.source')}</Badge>
            </CardHeader>
            <CardContent aria-busy={companyPending}>
              {renderCompanyInfo()}
              <button
                type="button"
                onClick={() =>
                  setLocation(`/stock-monitor/${encodeURIComponent(investment.ticker)}`)
                }
                className="mt-3.5 text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:shadow-focus"
              >
                {t('detail.openInMonitor')} →
              </button>
            </CardContent>
          </Card>

          <Card variant="flat">
            <CardContent className="pt-5">
              <h3 className="m-0 text-h3 text-ink">{t('detail.position.title')}</h3>
              <div className="mt-2">
                {kv(
                  t('detail.position.quantity'),
                  `${fmt.number(quantity, { maximumFractionDigits: 4 })} ${t('table.unit')}`
                )}
                {kv(
                  t('detail.position.avgPrice'),
                  `${fmt.money(holdingData.avgCost, avgCostCurrency, {
                    decimals: priceDecimals(holdingData.avgCost, avgCostCurrency),
                  })}${
                    avgCostCurrency !== currencyCode
                      ? ` ≈ ${fmt.money(position.averageCost, currencyCode, {
                          decimals: priceDecimals(position.averageCost, currencyCode),
                        })}`
                      : ''
                  }`
                )}
                {kv(
                  t('detail.position.currentPrice'),
                  fmt.money(currentPrice, currencyCode, {
                    decimals: priceDecimals(currentPrice, currencyCode),
                  })
                )}
                {kv(
                  t('detail.position.priceUpdated'),
                  fetchedAt !== null ? fmt.dateTime(fetchedAt) : undefined
                )}
              </div>
            </CardContent>
          </Card>
        </aside>
      </div>

      <BuyInvestmentModal
        open={showBuyModal}
        onOpenChange={setShowBuyModal}
        investment={holdingData}
      />
      <SellInvestmentModal
        open={showSellModal}
        onOpenChange={setShowSellModal}
        investment={holdingData}
      />
      <ManualPriceModal
        open={showPriceModal}
        onOpenChange={setShowPriceModal}
        investment={holdingData}
      />
      <ManualDividendModal
        open={showDividendModal}
        onOpenChange={setShowDividendModal}
        investment={holdingData}
      />
      <EditTransactionModal
        transaction={editingTransaction}
        open={editingTransaction !== null}
        onOpenChange={(open) => {
          if (!open) setEditingTransaction(null);
        }}
      />
      <ConfirmDeleteDialog
        open={pendingDelete !== null}
        onOpenChange={(open) => {
          if (!open) setPendingDelete(null);
        }}
        title={t('detail.confirmDeleteTransaction.title')}
        description={pendingDeleteDescription}
        onConfirm={() => {
          if (!pendingDelete) return;
          deleteTransaction.mutate(pendingDelete.id, { onSuccess: () => setPendingDelete(null) });
        }}
        isPending={deleteTransaction.isPending}
        confirmLabel={t('detail.deleteTransaction')}
      />
      <ConfirmDeleteDialog
        open={showDeleteDialog}
        onOpenChange={setShowDeleteDialog}
        title={t('detail.confirmDelete.title')}
        description={t('detail.confirmDelete.description', {
          name: investment.companyName,
          ticker: investment.ticker,
        })}
        onConfirm={() => deleteMutation.mutate()}
        isPending={deleteMutation.isPending}
        confirmLabel={t('actions.deletePosition')}
      />

      <Dialog open={showEditNameModal} onOpenChange={setShowEditNameModal}>
        <DialogContent className="max-w-[440px]">
          <DialogHeader>
            <DialogTitle>{t('detail.editName.title')}</DialogTitle>
            <DialogDescription>{t('detail.editName.description')}</DialogDescription>
          </DialogHeader>
          <form
            id="edit-name-form"
            className="space-y-[7px]"
            onSubmit={(e) => {
              e.preventDefault();
              if (editableName.trim()) updateNameMutation.mutate(editableName.trim());
            }}
          >
            <Label htmlFor="companyName">{t('detail.editName.label')}</Label>
            <Input
              id="companyName"
              value={editableName}
              onChange={(e) => setEditableName(e.target.value)}
              placeholder={investment.companyName}
              autoFocus
            />
          </form>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setShowEditNameModal(false)}>
              {tc('buttons.cancel')}
            </Button>
            <Button
              type="submit"
              form="edit-name-form"
              disabled={!editableName.trim() || updateNameMutation.isPending}
              loading={updateNameMutation.isPending}
            >
              {updateNameMutation.isPending ? tc('buttons.saving') : t('detail.editName.save')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
