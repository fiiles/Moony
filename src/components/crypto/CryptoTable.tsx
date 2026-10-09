import { useCallback, useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Ellipsis,
  Pencil,
  Search,
  ShoppingCart,
  Trash2,
  TrendingDown,
} from 'lucide-react';
import { priceDecimals } from '@shared/currencies';
import { matchesSearch } from '@/utils/holdings-search';
import { compareKnown, costPerUnit, type CryptoRow } from '@/utils/positions';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Checkbox } from '@/components/ui/checkbox';
import { Input, InputWrap } from '@/components/ui/input';
import { Progress } from '@/components/ui/progress';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { AssetLogo } from '@/components/common/AssetLogo';
import { cn } from '@/lib/utils';

/** A crypto position as the list shows it: the backend's figures plus its share. */
export interface CryptoTableRow extends CryptoRow {
  /** Share of the open crypto value, 0–1. */
  share: number;
  /** Day of the last sell when the position is closed. */
  closedAt?: number | null;
}

/** The cost per unit in the report currency, from the backend's cost basis. */
const unitCost = (h: CryptoRow) => costPerUnit(h.totalCost, h.quantity);

type SortColumn = 'name' | 'quantity' | 'avgCost' | 'currentPrice' | 'marketValue' | 'gainLoss';

interface CryptoTableProps {
  /** The report currency of the holdings' converted figures. */
  currency: string;
  holdings: CryptoTableRow[];
  onViewDetail: (holding: CryptoTableRow) => void;
  onBuy: (holding: CryptoTableRow) => void;
  onSell: (holding: CryptoTableRow) => void;
  onUpdatePrice: (holding: CryptoTableRow) => void;
  onDelete: (holding: CryptoTableRow) => void;
  isLoading?: boolean;
}

/**
 * Positions table of the crypto list (design system §6 Table, prototype
 * crypto.html): closed-positions toggle and search in the head, sortable
 * head, 62 px rows with the coin mark, native rate under the converted one,
 * a share bar, the return with its amount, "···" actions on hover.
 */
export function CryptoTable({
  currency,
  holdings,
  onViewDetail,
  onBuy,
  onSell,
  onUpdatePrice,
  onDelete,
  isLoading,
}: CryptoTableProps) {
  const { fxConvert } = useCurrency();
  const fmt = useFormat();
  // The rate in the report currency, converted with the response's quote snapshot
  const unitPrice = useCallback(
    (h: CryptoRow) => fxConvert(h.originalCurrentPrice, h.originalCurrency, currency),
    [fxConvert, currency]
  );
  const { t } = useTranslation('crypto');
  const [search, setSearch] = useState('');
  const [showClosed, setShowClosed] = useState(false);
  const [sortColumn, setSortColumn] = useState<SortColumn>('marketValue');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('desc');

  const handleSort = (column: SortColumn) => {
    if (sortColumn === column) setSortDirection((d) => (d === 'asc' ? 'desc' : 'asc'));
    else {
      setSortColumn(column);
      setSortDirection(column === 'name' ? 'asc' : 'desc');
    }
  };

  const filtered = useMemo(() => {
    const term = search.trim().toLowerCase();
    let rows = showClosed ? holdings : holdings.filter((h) => h.quantity > 0);
    if (term) rows = rows.filter((h) => matchesSearch(term, [h.ticker, h.name]));
    return [...rows].sort((a, b) => {
      let c = 0;
      switch (sortColumn) {
        case 'name':
          c = (a.name || '').localeCompare(b.name || '', undefined, { sensitivity: 'base' });
          break;
        case 'quantity':
          c = a.quantity - b.quantity;
          break;
        case 'avgCost':
          c = compareKnown(unitCost(a), unitCost(b));
          break;
        case 'currentPrice':
          c = compareKnown(unitPrice(a), unitPrice(b));
          break;
        case 'marketValue':
          c = compareKnown(a.marketValue, b.marketValue);
          break;
        case 'gainLoss':
          c = compareKnown(a.gainLoss, b.gainLoss);
          break;
      }
      return sortDirection === 'asc' ? c : -c;
    });
  }, [holdings, search, showClosed, sortColumn, sortDirection, unitPrice]);

  // Footer over the open rows shown: the values the backend could convert
  const openRows = filtered.filter((h) => h.quantity > 0);
  const total = openRows.reduce((sum, h) => sum + (h.marketValue ?? 0), 0);
  const closedCount = holdings.filter((h) => h.quantity <= 0).length;

  const SortHead = ({
    column,
    children,
    align = 'right',
    className,
  }: {
    column: SortColumn;
    children: ReactNode;
    align?: 'left' | 'right';
    className?: string;
  }) => {
    const active = sortColumn === column;
    const Icon = !active ? ArrowUpDown : sortDirection === 'asc' ? ArrowUp : ArrowDown;
    return (
      <TableHead className={cn(align === 'right' && 'text-right', className)}>
        <button
          type="button"
          onClick={() => handleSort(column)}
          aria-sort={active ? (sortDirection === 'asc' ? 'ascending' : 'descending') : undefined}
          className={cn(
            'inline-flex items-center gap-1 rounded-r1 text-thead uppercase transition-colors duration-fast hover:text-ink focus-visible:outline-none focus-visible:shadow-focus',
            active && 'text-ink'
          )}
        >
          {children}
          <Icon className={cn('size-3', !active && 'opacity-60')} aria-hidden />
        </button>
      </TableHead>
    );
  };

  const price = (value: number, currency: string) =>
    fmt.money(value, currency, { decimals: priceDecimals(value, currency) });

  return (
    <Card
      variant="table"
      className={cn('transition-opacity duration-base', isLoading && 'opacity-50')}
    >
      <CardHeader>
        <div>
          <CardTitle>{t('table.title')}</CardTitle>
          <CardDescription>{t('table.subtitle', { currency })}</CardDescription>
        </div>
        <div className="flex items-center gap-4">
          {closedCount > 0 && (
            <label className="inline-flex cursor-pointer items-center gap-[9px] text-table font-500 text-ink-2">
              <Checkbox checked={showClosed} onCheckedChange={(v) => setShowClosed(v === true)} />
              {t('table.showClosed')}
            </label>
          )}
          <InputWrap icon={<Search />} className="w-60">
            <Input
              className="h-[35px] text-table"
              placeholder={t('table.search')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              data-testid="input-search-crypto"
            />
          </InputWrap>
        </div>
      </CardHeader>

      <Table>
        <TableHeader>
          <TableRow>
            <SortHead column="name" align="left" className="w-[24%]">
              {t('table.asset')}
            </SortHead>
            <SortHead column="quantity">{t('table.quantity')}</SortHead>
            <SortHead column="avgCost">{t('table.avgCost')}</SortHead>
            <SortHead column="currentPrice">{t('table.rate')}</SortHead>
            <SortHead column="marketValue">{t('table.value')}</SortHead>
            <TableHead className="text-right">{t('table.share')}</TableHead>
            <SortHead column="gainLoss">{t('table.return')}</SortHead>
            <TableHead className="w-10">
              <span className="sr-only">{t('table.actions')}</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {filtered.length === 0 ? (
            <TableRow className="hover:bg-transparent">
              <TableCell colSpan={8} className="py-12">
                <div className="grid place-items-center text-center text-ink-3">
                  <Search className="mb-3 size-7 text-ink-5" aria-hidden />
                  <h3 className="mb-1.5 text-h3 text-ink">{t('table.noResults.title')}</h3>
                  <p className="m-0 max-w-[320px] text-table">
                    {showClosed || closedCount === 0
                      ? t('table.noResults.description')
                      : t('table.noResults.closedHidden')}
                  </p>
                </div>
              </TableCell>
            </TableRow>
          ) : (
            filtered.map((h) => {
              const closed = h.quantity <= 0;
              const nativePrice = h.originalCurrentPrice;
              const rate = unitPrice(h);
              const cost = unitCost(h);
              const nativeCurrency =
                rate !== null && h.originalCurrency !== currency ? h.originalCurrency : null;
              return (
                <TableRow
                  key={h.id}
                  className="h-[62px] cursor-pointer"
                  onClick={() => onViewDetail(h)}
                  tabIndex={0}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') onViewDetail(h);
                  }}
                >
                  <TableCell>
                    <div className="flex min-w-0 items-center gap-[10px] text-ink">
                      <AssetLogo
                        ticker={h.ticker}
                        type="crypto"
                        variant={closed ? 'soft' : 'series'}
                      />
                      <div className="min-w-0">
                        <b className="block truncate text-table font-650">{h.name}</b>
                        <small className="mt-[3px] block text-micro font-500 text-ink-4">
                          {h.ticker} ·{' '}
                          {closed && h.closedAt
                            ? t('table.closed', { date: fmt.day(h.closedAt) })
                            : h.isManualPrice
                              ? t('table.manualSource')
                              : t('table.source')}
                        </small>
                      </div>
                    </div>
                  </TableCell>
                  <TableCell className="text-right num">
                    {fmt.number(h.quantity, { maximumFractionDigits: 8 })} {h.ticker}
                  </TableCell>
                  <TableCell className="text-right num">
                    {closed || cost === null ? (
                      <span className="text-ink-4">—</span>
                    ) : (
                      price(cost, currency)
                    )}
                  </TableCell>
                  <TableCell className="text-right num">
                    <span className="inline-flex items-center justify-end gap-1.5">
                      {h.isManualPrice && (
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Badge variant="outline" className="cursor-help">
                              {t('badges.manual')}
                            </Badge>
                          </TooltipTrigger>
                          <TooltipContent>{t('tooltip.manualPrice')}</TooltipContent>
                        </Tooltip>
                      )}
                      {rate !== null
                        ? price(rate, currency)
                        : price(nativePrice, h.originalCurrency)}
                    </span>
                    {nativeCurrency && nativePrice > 0 ? (
                      <small className="mt-[3px] block text-micro font-500 text-ink-4">
                        {price(nativePrice, nativeCurrency)}
                      </small>
                    ) : null}
                  </TableCell>
                  <TableCell className="text-right font-650 text-ink num">
                    {closed || h.marketValue === null ? (
                      <span className="font-500 text-ink-4">—</span>
                    ) : (
                      fmt.money(h.marketValue, currency, { decimals: 0 })
                    )}
                  </TableCell>
                  <TableCell className="text-right">
                    {closed ? (
                      <span className="text-ink-4">—</span>
                    ) : (
                      <span className="inline-flex items-center justify-end gap-2">
                        <Progress value={h.share * 100} className="w-[46px]" />
                        <span className="min-w-[34px] num">{fmt.percent(h.share, 0)}</span>
                      </span>
                    )}
                  </TableCell>
                  <TableCell className="text-right num">
                    {closed ? (
                      <span className="text-ink-4">
                        {h.realizedGain
                          ? t('table.realized', {
                              amount: fmt.money(h.realizedGain, currency, {
                                signed: true,
                                decimals: 0,
                              }),
                            })
                          : '—'}
                      </span>
                    ) : h.gainLoss === null ? (
                      <span className="text-ink-4">—</span>
                    ) : (
                      <span
                        className={cn(
                          'block font-650',
                          h.gainLoss >= 0 ? 'text-gain' : 'text-loss'
                        )}
                      >
                        {fmt.money(h.gainLoss, currency, { signed: true, decimals: 0 })}
                        <small className="mt-[3px] block text-micro font-600">
                          {fmt.percent((h.gainLossPercent ?? 0) / 100, 1, { signed: true })}
                        </small>
                      </span>
                    )}
                  </TableCell>
                  <TableCell className="text-right">
                    <div data-row-actions onClick={(e) => e.stopPropagation()}>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon-sm" aria-label={t('table.actions')}>
                            <Ellipsis />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem onSelect={() => onBuy(h)}>
                            <ShoppingCart />
                            {t('actions.buy')}
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => onSell(h)} disabled={closed}>
                            <TrendingDown />
                            {t('actions.sell')}
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => onUpdatePrice(h)}>
                            <Pencil />
                            {t('actions.updatePrice')}
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem variant="danger" onSelect={() => onDelete(h)}>
                            <Trash2 />
                            {t('actions.delete')}
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </div>
                  </TableCell>
                </TableRow>
              );
            })
          )}
        </TableBody>
      </Table>
      <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
        <span>
          {t('table.footer.count', { count: openRows.length })} · {t('table.footer.total')}{' '}
          <span className="num">{fmt.money(total, currency, { decimals: 0 })}</span>
        </span>
        <span>
          {t('table.footer.rates', { currency })}
          {closedCount > 0 && ` · ${t('table.footer.closed', { count: closedCount })}`}
        </span>
      </div>
    </Card>
  );
}
