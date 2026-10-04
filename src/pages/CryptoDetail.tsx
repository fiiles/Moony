import { useMemo, useState } from 'react';
import { useLocation, useRoute } from 'wouter';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Ellipsis, Pencil, Plus, RefreshCw, Trash2 } from 'lucide-react';
import type { CryptoTransaction } from '@shared/schema';
import type { CryptoInvestmentWithPrice } from '@shared/types/extended-types';
import { type CurrencyCode, priceDecimals } from '@shared/currencies';
import {
  calculatePositionCostBasis,
  calculateRealizedGains,
  mapCryptoInvestmentToHolding,
} from '@shared/calculations';
import { cryptoApi, priceApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useDatedConvert } from '@/hooks/use-dated-convert';
import { useCryptoTransactionMutations } from '@/hooks/use-crypto-mutations';
import { useShellPage } from '@/components/shell/shell-context';
import { formatAmountWithCode } from '@/utils/format-amount';
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
  const { formatCurrencyRaw, currencyCode, convert } = useCurrency();
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const preferredCurrency = currencyCode as CurrencyCode;

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
  const { data: allCrypto, isLoading } = useQuery<CryptoInvestmentWithPrice[]>({
    queryKey: ['crypto'],
    queryFn: () => cryptoApi.getAll(),
    staleTime: 60 * 1000,
  });
  const crypto = useMemo(() => allCrypto?.find((c) => c.id === id) ?? null, [allCrypto, id]);

  const { data: transactions } = useQuery<CryptoTransaction[]>({
    queryKey: ['crypto-transactions', id],
    queryFn: () => cryptoApi.getTransactions(id!),
    enabled: !!id,
  });

  // Date-aware conversion over the transactions' range (ADR 0001)
  const convertAt = useDatedConvert(transactions);

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
      await queryClient.invalidateQueries({ queryKey: ['crypto'] });
      toast(t('detail.pricesRefreshed'));
    } catch (error) {
      toast.error(tc('status.error'), { description: String(error) });
    } finally {
      setIsRefreshing(false);
    }
  };

  // Display-currency price of one position from its native rate at today's FX
  const displayPrice = (inv: CryptoInvestmentWithPrice) => {
    const native = parseFloat(String(inv.originalPrice)) || 0;
    return native > 0
      ? convert(native, (inv.currency || 'USD') as CurrencyCode, preferredCurrency)
      : convert(parseFloat(String(inv.currentPrice)) || 0, 'CZK', preferredCurrency);
  };

  const quantity = parseFloat(String(crypto?.quantity)) || 0;
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
  // Realized result of each sale: the running total before and after it
  const realizedBySale = useMemo(() => {
    const ordered = [...(transactions ?? [])].sort((a, b) => a.transactionDate - b.transactionDate);
    const map = new Map<string, number>();
    ordered.forEach((tx, i) => {
      if (tx.type !== 'sell') return;
      const after = calculateRealizedGains(ordered.slice(0, i + 1), convertAt, preferredCurrency);
      const before =
        i > 0 ? calculateRealizedGains(ordered.slice(0, i), convertAt, preferredCurrency) : 0;
      map.set(tx.id, after - before);
    });
    return map;
  }, [transactions, convertAt, preferredCurrency]);

  const nativeCurrency = (crypto?.currency || 'USD') as CurrencyCode;
  const nativePrice = parseFloat(String(crypto?.originalPrice)) || 0;
  const currentPrice = crypto ? displayPrice(crypto) : 0;
  const currentValue = quantity * currentPrice;
  const totalInvested = position.costBasis;
  const unrealized = currentValue - totalInvested;
  const unrealizedPct = totalInvested > 0 ? unrealized / totalInvested : 0;

  const cryptoTotal = (allCrypto ?? []).reduce((sum, inv) => {
    const q = parseFloat(String(inv.quantity)) || 0;
    return q > 0 ? sum + q * displayPrice(inv) : sum;
  }, 0);
  const share = cryptoTotal > 0 ? currentValue / cryptoTotal : 0;

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
  const firstBuyPrice = firstBuy
    ? convertAt(
        parseFloat(firstBuy.pricePerUnit) || 0,
        (firstBuy.currency || currencyCode) as CurrencyCode,
        preferredCurrency,
        firstBuy.transactionDate
      )
    : 0;
  const rateChange = firstBuyPrice > 0 ? currentPrice / firstBuyPrice - 1 : 0;

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
  const txDisplayPrice = (tx: CryptoTransaction) =>
    convertAt(
      parseFloat(tx.pricePerUnit) || 0,
      (tx.currency || currencyCode) as CurrencyCode,
      preferredCurrency,
      tx.transactionDate
    );
  const boughtTotal = buys.reduce(
    (s, tx) => s + (parseFloat(tx.quantity) || 0) * txDisplayPrice(tx),
    0
  );
  const soldTotal = sells.reduce(
    (s, tx) => s + (parseFloat(tx.quantity) || 0) * txDisplayPrice(tx),
    0
  );

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

  const holdingData = mapCryptoInvestmentToHolding(
    crypto.id,
    crypto.ticker,
    crypto.name || crypto.ticker,
    quantity,
    position.averageCost,
    currentPrice,
    crypto.fetchedAt,
    crypto.isManualPrice,
    crypto.coingeckoId,
    crypto.originalPrice,
    crypto.currency
  );
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
          totalInvested > 0
            ? t('detail.avgBuyPrice', { price: priceOf(position.averageCost, currencyCode) })
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

      <PositionHero
        type="crypto"
        ticker={crypto.ticker}
        currentValue={currentValue}
        currentCost={totalInvested}
        transactions={transactions ?? []}
        convertAt={convertAt}
        hotEventId={hotEventId}
        onHotEventChange={setHotEventId}
        onEventClick={scrollToRow}
        isRefreshing={isRefreshing}
        unit={crypto.ticker}
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
              ? `${fmt.percent(unrealizedPct, 1, { signed: true })} ${t('detail.vsInvested')}`
              : undefined
          }
        />
        <HeroValue
          compact
          label={t('detail.rate')}
          value={priceOf(currentPrice, currencyCode)}
          delta={
            crypto.isManualPrice ? <Badge variant="outline">{t('badges.manual')}</Badge> : undefined
          }
          deltaNote={
            nativePrice > 0 && nativeCurrency !== currencyCode
              ? priceOf(nativePrice, nativeCurrency)
              : undefined
          }
        />
      </PositionHero>

      <Stats className="mt-[17px]">
        <Stat
          label={t('detail.invested')}
          value={formatCurrencyRaw(totalInvested)}
          note={`${t('detail.quantityUnits', { quantity: unitQty(quantity), ticker: crypto.ticker })} · ${t(
            'detail.avgShort',
            { price: priceOf(position.averageCost, currencyCode) }
          )}`}
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
                      ? t('detail.priceNative', { currency: currencyCode })
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
                      <TableCell className="text-right num">
                        {priceOf(priceInDisplay, currencyCode)}
                      </TableCell>
                      <TableCell className="text-right font-650 text-ink num">
                        {formatCurrencyRaw(txQty * priceInDisplay)}
                        {saleResult !== undefined && (
                          <small
                            className={`mt-[3px] block text-micro font-600 ${saleResult >= 0 ? 'text-gain' : 'text-loss'}`}
                          >
                            {t('detail.realizedRow', {
                              amount: fmt.money(saleResult, currencyCode, {
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
              {boughtTotal > 0 &&
                ` · ${t('detail.ledger.bought', { amount: formatCurrencyRaw(boughtTotal) })}`}
              {soldTotal > 0 &&
                ` · ${t('detail.ledger.sold', { amount: formatCurrencyRaw(soldTotal) })}`}
            </span>
            <span>{t('detail.ledger.rates', { currency: currencyCode })}</span>
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
                totalInvested > 0 ? priceOf(position.averageCost, currencyCode) : undefined
              )}
              {kv(t('detail.position.currentPrice'), priceOf(currentPrice, currencyCode))}
              {nativeCurrency !== currencyCode &&
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
                      firstPrice: priceOf(firstBuyPrice, currencyCode),
                      positionChange: fmt.percent(unrealizedPct, 1, { signed: true }),
                      realizedPart:
                        realized !== 0
                          ? t('detail.sinceFirst.realizedPart', {
                              amount: fmt.money(realized, currencyCode, {
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
