import { useMemo, useState } from 'react';
import { invalidateAfterPriceRefresh } from '@/lib/price-refresh';
import { useLocation, useRoute } from 'wouter';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Ellipsis, Pencil, Plus, RefreshCw, Trash2 } from 'lucide-react';
import type { CryptoPositions, CryptoTransaction } from '@shared/schema';
import { type CurrencyCode, priceDecimals } from '@shared/currencies';
import { calculateRealizedGains, type DatedConvertFn } from '@shared/calculations';
import { cryptoApi, priceApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useTransactionFx } from '@/hooks/use-fx';
import { useCryptoTransactionMutations } from '@/hooks/use-crypto-mutations';
import { cn } from '@/lib/utils';
import { useShellPage } from '@/components/shell/shell-context';
import { formatAmountWithCode } from '@/utils/format-amount';
import { amountOf, costPerUnit, toCryptoRow } from '@/utils/positions';
import { monthsBetween } from '@/utils/duration';
import { utcDayFloor } from '@/utils/chart-axis';
import { BackLink, PageHead } from '@/components/shell/PageHead';
import { HeroValue } from '@/components/common/HeroCard';
import { Stat, Stats } from '@/components/common/Stat';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Segmented } from '@/components/ui/segmented';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { PositionHero } from '@/components/stocks/PositionHero';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';
import { BuyCryptoModal } from '@/components/crypto/BuyCryptoModal';
import { SellCryptoModal } from '@/components/crypto/SellCryptoModal';
import { UpdateCryptoPriceModal } from '@/components/crypto/UpdateCryptoPriceModal';

type LedgerFilter = 'all' | 'buy' | 'sell';

/**
 * Crypto position detail (design system §7 Detail, prototype
 * crypto-detail.html): the same anatomy as the stock detail — hero with the
 * cost-basis step line and buy/sell events, four stats incl. the holding
 * period and the share of the crypto total, ledger with per-sale realized
 * results, and a "since the first purchase" card comparing the coin's rate
 * move with the position's return.
 */
export default function CryptoDetail() {
  const [, params] = useRoute('/crypto/:id');
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const id = params?.id;
  const { currencyCode, fxRevision, convert } = useCurrency();
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();

  const [showBuyModal, setShowBuyModal] = useState(false);
  const [showSellModal, setShowSellModal] = useState(false);
  const [showPriceModal, setShowPriceModal] = useState(false);
  const [showDeleteDialog, setShowDeleteDialog] = useState(false);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<CryptoTransaction | null>(null);
  const [filter, setFilter] = useState<LedgerFilter>('all');
  const [hotEventId, setHotEventId] = useState<string | null>(null);
  const { deleteTransaction } = useCryptoTransactionMutations();

  // The list query is the source: one position and the crypto total come from the same rows
  const { data: positions, isLoading } = useQuery<CryptoPositions>({
    queryKey: ['crypto', currencyCode, fxRevision],
    queryFn: () => cryptoApi.getAll(currencyCode),
    staleTime: 60 * 1000,
  });
  const crypto = useMemo(() => positions?.items.find((c) => c.id === id) ?? null, [positions, id]);
  const reportCurrency = positions?.currency ?? currencyCode;

  const { data: transactions } = useQuery<CryptoTransaction[]>({
    queryKey: ['crypto-transactions', id],
    queryFn: () => cryptoApi.getTransactions(id!),
    enabled: !!id,
  });

  // Each ledger row and per-sale result converts at the rates of its own day (common-day rule);
  // the position's own figures and the hero come from the backend.
  const { ready: ratesReady, convertAt: convertTxAt } = useTransactionFx(transactions);
  const missingRateTitle = (currency: string) =>
    tc('fxQuality.missing', { currency: reportCurrency, currencies: currency });
  // A day without rates gives NaN, so a result built on it is left out rather than shown wrong
  const convertAt = useMemo<DatedConvertFn>(
    () => (amount, from, to, ts) => convertTxAt(amount, from, to, ts) ?? Number.NaN,
    [convertTxAt]
  );

  const deleteMutation = useMutation({
    mutationFn: () => cryptoApi.delete(id!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['crypto'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['all-crypto-transactions'] });
      toast(t('toast.deleted'));
      setLocation('/crypto');
    },
    onError: (error) => {
      toast.error(tc('status.error'), { description: String(error) });
    },
  });

  const handleRefresh = async () => {
    setIsRefreshing(true);
    try {
      await priceApi.refreshCryptoPrices();
      await invalidateAfterPriceRefresh(queryClient);
      toast(t('detail.pricesRefreshed'));
    } catch (error) {
      toast.error(tc('status.error'), { description: String(error) });
    } finally {
      setIsRefreshing(false);
    }
  };

  // Position numbers as the backend values them in the report currency (ADR 0011)
  const row = crypto ? toCryptoRow(crypto) : null;
  const quantity = row?.quantity ?? 0;
  const totalInvested = row?.totalCost ?? null;
  const averageCost = costPerUnit(totalInvested, quantity);
  const realized = row?.realizedGain ?? null;
  const resultCurrency = reportCurrency as CurrencyCode;
  // Realized result of each sale: the running total before and after it
  const realizedBySale = useMemo(() => {
    const ordered = [...(transactions ?? [])].sort((a, b) => a.transactionDate - b.transactionDate);
    const map = new Map<string, number>();
    ordered.forEach((tx, i) => {
      if (tx.type !== 'sell') return;
      const after = calculateRealizedGains(ordered.slice(0, i + 1), convertAt, resultCurrency);
      const before =
        i > 0 ? calculateRealizedGains(ordered.slice(0, i), convertAt, resultCurrency) : 0;
      const result = after - before;
      if (Number.isFinite(result)) map.set(tx.id, result);
    });
    return map;
  }, [transactions, convertAt, resultCurrency]);

  const nativeCurrency = row?.originalCurrency ?? reportCurrency;
  const nativePrice = row?.originalCurrentPrice ?? 0;
  const currentPrice = convert(nativePrice, nativeCurrency, reportCurrency);
  const currentValue = row?.marketValue ?? null;
  const unrealized = row?.gainLoss ?? null;
  const unrealizedPct = (row?.gainLossPercent ?? 0) / 100;
  const cryptoTotal = amountOf(positions?.totals.value) ?? 0;
  const share = cryptoTotal > 0 && currentValue !== null ? currentValue / cryptoTotal : 0;
  const tone = (v: number | null) =>
    v !== null && v > 0 ? 'gain' : v !== null && v < 0 ? 'loss' : 'neutral';
  const money = (v: number | null, opts?: { signed?: boolean }) =>
    v === null ? '—' : fmt.money(v, reportCurrency, { decimals: 0, ...opts });

  const buys = (transactions ?? []).filter((tx) => tx.type === 'buy');
  const sells = (transactions ?? []).filter((tx) => tx.type === 'sell');
  const firstBuy = buys.reduce<CryptoTransaction | null>(
    (min, tx) => (min === null || tx.transactionDate < min.transactionDate ? tx : min),
    null
  );
  const lastBuy = buys.reduce<number | null>(
    (max, tx) => (max === null || tx.transactionDate > max ? tx.transactionDate : max),
    null
  );
  const nowSec = Math.floor(Date.now() / 1000);
  const held = firstBuy ? monthsBetween(firstBuy.transactionDate, nowSec) : null;
  const durationText = held
    ? held.totalMonths === 0
      ? t('detail.duration.lessThanMonth')
      : [
          held.years > 0 ? t('detail.duration.years', { count: held.years }) : null,
          held.months > 0 ? t('detail.duration.months', { count: held.months }) : null,
        ]
          .filter(Boolean)
          .join(' ')
    : null;
  const firstBuyNative = firstBuy ? parseFloat(firstBuy.pricePerUnit) || 0 : 0;
  const firstBuyCurrency = firstBuy?.currency || currencyCode;
  const firstBuyPrice = firstBuy
    ? convertTxAt(firstBuyNative, firstBuyCurrency, reportCurrency, firstBuy.transactionDate)
    : null;
  const rateChange =
    firstBuyPrice !== null && firstBuyPrice > 0 && currentPrice !== null
      ? currentPrice / firstBuyPrice - 1
      : 0;

  // Top bar: entity name and freshness of the rate
  const fetchedAt = crypto?.fetchedAt ? Number(crypto.fetchedAt) : null;
  const fetchedToday =
    fetchedAt !== null && utcDayFloor(fetchedAt) === utcDayFloor(Date.now() / 1000);
  useShellPage({
    crumb: crypto?.name || crypto?.ticker,
    status:
      fetchedAt !== null
        ? {
            text: t('detail.status.rateUpdated', {
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
  const txDisplayPrice = (tx: CryptoTransaction): number | null =>
    convertTxAt(
      parseFloat(tx.pricePerUnit) || 0,
      tx.currency || currencyCode,
      reportCurrency,
      tx.transactionDate
    );
  // A total is shown only when every one of its rows converted
  const totalOf = (rows: CryptoTransaction[]): number | null =>
    rows.reduce<number | null>((sum, tx) => {
      const price = txDisplayPrice(tx);
      return sum === null || price === null ? null : sum + (parseFloat(tx.quantity) || 0) * price;
    }, 0);
  const boughtTotal = totalOf(buys);
  const soldTotal = totalOf(sells);

  const scrollToRow = (ids: string[]) => {
    document
      .getElementById(`tx-${ids[0]}`)
      ?.scrollIntoView({ behavior: 'smooth', block: 'center' });
  };

  const pendingDeleteDescription = pendingDelete
    ? t('detail.confirmDeleteTransaction.description', {
        type: (pendingDelete.type === 'buy' ? tc('buttons.buy') : tc('buttons.sell')).toLowerCase(),
        quantity: fmt.number(Number(pendingDelete.quantity), { maximumFractionDigits: 8 }),
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
        <BackLink href="/crypto">{t('detail.backToList')}</BackLink>
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

  if (!crypto) {
    return (
      <>
        <BackLink href="/crypto">{t('detail.backToList')}</BackLink>
        <p className="mt-8 text-center text-ink-3">{t('detail.notFound')}</p>
      </>
    );
  }

  const holdingData = row;
  const priceOf = (value: number, currency: string) =>
    fmt.money(value, currency, { decimals: priceDecimals(value, currency) });
  const unitQty = (value: number) => fmt.number(value, { maximumFractionDigits: 8 });

  const kv = (label: string, value: string | null | undefined) => (
    <div className="grid grid-cols-[1fr_auto] gap-x-4 gap-y-2.5 border-b border-line-soft py-[9px] text-table text-ink-3 last:border-0">
      <span>{label}</span>
      <b className="font-600 text-ink num">{value || '—'}</b>
    </div>
  );

  return (
    <>
      <BackLink href="/crypto">{t('detail.backToList')}</BackLink>
      <PageHead
        eyebrow={[
          crypto.ticker,
          crypto.isManualPrice
            ? t('detail.manualSource')
            : crypto.coingeckoId
              ? t('detail.source')
              : null,
          nativeCurrency,
        ]}
        title={crypto.name || crypto.ticker}
        description={[
          t('detail.quantityUnits', { quantity: unitQty(quantity), ticker: crypto.ticker }),
          averageCost !== null && averageCost > 0
            ? t('detail.avgBuyPrice', { price: priceOf(averageCost, reportCurrency) })
            : null,
          lastBuy !== null ? t('detail.lastBuy', { date: fmt.day(lastBuy) }) : null,
        ]
          .filter(Boolean)
          .join(' · ')}
        actions={
          <>
            <Button
              variant="outline"
              onClick={handleRefresh}
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
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="danger" onSelect={() => setShowDeleteDialog(true)}>
                  <Trash2 />
                  {t('detail.menu.delete')}
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

      {/* The list's quality, shown when this position is one of those not converted */}
      {(currentValue === null || totalInvested === null) && (
        <ConversionQualityNote quality={positions?.fxQuality} currency={reportCurrency} />
      )}

      <PositionHero
        type="crypto"
        ticker={crypto.ticker}
        currentValue={currentValue}
        currentCost={totalInvested}
        transactions={transactions ?? []}
        hotEventId={hotEventId}
        onHotEventChange={setHotEventId}
        onEventClick={scrollToRow}
        isRefreshing={isRefreshing}
        unit={crypto.ticker}
      >
        <HeroValue
          label={t('detail.positionValue')}
          value={money(currentValue)}
          tone={tone(unrealized)}
          delta={
            unrealized !== null && totalInvested !== null && totalInvested > 0
              ? `${unrealized >= 0 ? '↗' : '↘'} ${money(unrealized, { signed: true })}`
              : undefined
          }
          deltaNote={
            unrealized !== null && totalInvested !== null && totalInvested > 0
              ? `${fmt.percent(unrealizedPct, 1, { signed: true })} ${t('detail.vsInvested')}`
              : undefined
          }
        />
        <HeroValue
          compact
          label={t('detail.rate')}
          value={
            currentPrice !== null
              ? priceOf(currentPrice, reportCurrency)
              : priceOf(nativePrice, nativeCurrency)
          }
          delta={
            crypto.isManualPrice ? <Badge variant="outline">{t('badges.manual')}</Badge> : undefined
          }
          deltaNote={
            currentPrice !== null && nativePrice > 0 && nativeCurrency !== reportCurrency
              ? priceOf(nativePrice, nativeCurrency)
              : undefined
          }
        />
      </PositionHero>

      <Stats className="mt-[17px]">
        <Stat
          label={t('detail.invested')}
          value={money(totalInvested)}
          note={`${t('detail.quantityUnits', { quantity: unitQty(quantity), ticker: crypto.ticker })} · ${t(
            'detail.avgShort',
            { price: averageCost !== null ? priceOf(averageCost, reportCurrency) : '—' }
          )}`}
        />
        <Stat
          label={t('detail.unrealized')}
          value={money(unrealized, { signed: true })}
          tone={tone(unrealized)}
          note={`${fmt.percent(unrealizedPct, 1, { signed: true })} ${t('detail.vsInvested')}`}
        />
        <Stat
          label={t('detail.realized')}
          value={money(realized, { signed: true })}
          tone={tone(realized)}
          noteTone="neutral"
          note={sells.length > 0 ? t('detail.sells', { count: sells.length }) : t('detail.noSells')}
        />
        <Stat
          label={t('detail.held')}
          value={durationText ?? '—'}
          note={
            firstBuy
              ? `${t('detail.heldSince', { date: fmt.day(firstBuy.transactionDate) })} · ${t('detail.shareOfCrypto', { share: fmt.percent(share, 0) })}`
              : t('detail.sinceFirst.none')
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
                      ? t('detail.priceNative', { currency: reportCurrency })
                      : t('detail.transactions.price')}
                  </TableHead>
                  <TableHead className="text-right">{t('detail.transactions.total')}</TableHead>
                  <TableHead className="w-12">
                    <span className="sr-only">{tc('labels.actions')}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {ledger.map((tx) => {
                  const txQty = parseFloat(tx.quantity) || 0;
                  const txPrice = parseFloat(tx.pricePerUnit) || 0;
                  const txCurrency = (tx.currency || currencyCode) as CurrencyCode;
                  const priceInDisplay = txDisplayPrice(tx);
                  // No rate for the day: the row keeps its own currency and says so
                  const unconverted = priceInDisplay === null && ratesReady;
                  const isSell = tx.type === 'sell';
                  const saleResult = isSell ? realizedBySale.get(tx.id) : undefined;
                  return (
                    <TableRow
                      key={tx.id}
                      id={`tx-${tx.id}`}
                      className="h-[52px]"
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
                        {unitQty(txQty)} {tx.ticker}
                      </TableCell>
                      {hasMixedCurrencies && (
                        <TableCell className="text-right num">
                          {priceOf(txPrice, txCurrency)}
                        </TableCell>
                      )}
                      <TableCell
                        className={cn('text-right num', unconverted && 'text-loss')}
                        title={unconverted ? missingRateTitle(txCurrency) : undefined}
                      >
                        {priceInDisplay !== null
                          ? priceOf(priceInDisplay, reportCurrency)
                          : priceOf(txPrice, txCurrency)}
                      </TableCell>
                      <TableCell
                        className={cn(
                          'text-right font-650 num',
                          unconverted ? 'text-loss' : 'text-ink'
                        )}
                        title={unconverted ? missingRateTitle(txCurrency) : undefined}
                      >
                        {priceInDisplay !== null
                          ? fmt.money(txQty * priceInDisplay, reportCurrency, { decimals: 0 })
                          : fmt.money(txQty * txPrice, txCurrency, { decimals: 0 })}
                        {saleResult !== undefined && (
                          <small
                            className={`mt-[3px] block text-micro font-600 ${saleResult >= 0 ? 'text-gain' : 'text-loss'}`}
                          >
                            {t('detail.realizedRow', {
                              amount: fmt.money(saleResult, reportCurrency, {
                                signed: true,
                                decimals: 0,
                              }),
                            })}
                          </small>
                        )}
                      </TableCell>
                      <TableCell className="text-right">
                        <div data-row-actions>
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
              {boughtTotal !== null &&
                boughtTotal > 0 &&
                ` · ${t('detail.ledger.bought', { amount: fmt.money(boughtTotal, reportCurrency, { decimals: 0 }) })}`}
              {soldTotal !== null &&
                soldTotal > 0 &&
                ` · ${t('detail.ledger.sold', { amount: fmt.money(soldTotal, reportCurrency, { decimals: 0 }) })}`}
            </span>
            <span>{t('detail.ledger.rates', { currency: reportCurrency })}</span>
          </div>
        </Card>

        <aside className="grid gap-[14px]">
          <Card>
            <CardHeader className="pb-1.5">
              <CardTitle className="text-h3">{t('detail.position.title')}</CardTitle>
              <Badge variant="outline">
                {crypto.isManualPrice ? t('detail.manualSource') : t('detail.source')}
              </Badge>
            </CardHeader>
            <CardContent>
              {kv(
                t('detail.position.quantity'),
                t('detail.quantityUnits', { quantity: unitQty(quantity), ticker: crypto.ticker })
              )}
              {kv(
                t('detail.position.avgPrice'),
                averageCost !== null && averageCost > 0
                  ? priceOf(averageCost, reportCurrency)
                  : undefined
              )}
              {kv(
                t('detail.position.currentPrice'),
                currentPrice !== null ? priceOf(currentPrice, reportCurrency) : undefined
              )}
              {nativeCurrency !== reportCurrency &&
                nativePrice > 0 &&
                kv(
                  t('detail.position.nativePrice', { currency: nativeCurrency }),
                  priceOf(nativePrice, nativeCurrency)
                )}
              {kv(
                t('detail.position.shareOfCrypto'),
                quantity > 0 ? fmt.percent(share, 0) : undefined
              )}
              {kv(
                t('detail.position.priceUpdated'),
                fetchedAt !== null ? fmt.dateTime(fetchedAt) : undefined
              )}
              <button
                type="button"
                onClick={() => setShowPriceModal(true)}
                className="mt-3.5 text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:shadow-focus"
              >
                {crypto.isManualPrice
                  ? t('detail.position.removeManual')
                  : t('detail.position.setManual')}
              </button>
            </CardContent>
          </Card>

          <Card variant="flat">
            <CardContent className="pt-5">
              <h3 className="m-0 text-h3 text-ink">{t('detail.sinceFirst.title')}</h3>
              <p className="mb-0 mt-1.5 text-caption leading-[1.5] text-ink-3">
                {firstBuy && durationText
                  ? t('detail.sinceFirst.text', {
                      duration: durationText,
                      ticker: crypto.ticker,
                      rateChange: fmt.percent(rateChange, 1, { signed: true }),
                      firstPrice:
                        firstBuyPrice !== null
                          ? priceOf(firstBuyPrice, reportCurrency)
                          : priceOf(firstBuyNative, firstBuyCurrency),
                      positionChange: fmt.percent(unrealizedPct, 1, { signed: true }),
                      realizedPart:
                        realized !== null && realized !== 0
                          ? t('detail.sinceFirst.realizedPart', {
                              amount: fmt.money(realized, reportCurrency, {
                                signed: true,
                                decimals: 0,
                              }),
                            })
                          : '',
                    })
                  : t('detail.sinceFirst.none')}
              </p>
            </CardContent>
          </Card>
        </aside>
      </div>

      <BuyCryptoModal open={showBuyModal} onOpenChange={setShowBuyModal} investment={holdingData} />
      <SellCryptoModal
        open={showSellModal}
        onOpenChange={setShowSellModal}
        investment={holdingData}
      />
      <UpdateCryptoPriceModal
        open={showPriceModal}
        onOpenChange={setShowPriceModal}
        investment={holdingData}
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
        title={t('confirmDelete.title')}
        description={t('confirmDelete.description', {
          name: crypto.name || crypto.ticker,
          ticker: crypto.ticker,
        })}
        onConfirm={() => deleteMutation.mutate()}
        isPending={deleteMutation.isPending}
        confirmLabel={t('actions.delete')}
      />
    </>
  );
}
