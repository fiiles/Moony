import { useMemo, useState, type ReactNode } from 'react';
import { Link } from 'wouter';
import type { StockTag } from '@shared/schema';
import { matchesSearch } from '@/utils/holdings-search';
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
  Tag,
} from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardHeader, CardTitle } from '@/components/ui/card';
import { Checkbox } from '@/components/ui/checkbox';
import { Input, InputWrap } from '@/components/ui/input';
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
import type { HoldingData } from '@/utils/stocks';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { priceDecimals } from '@shared/currencies';
import { cn } from '@/lib/utils';

type SortColumn =
  'name' | 'quantity' | 'avgCost' | 'currentPrice' | 'marketValue' | 'gainLoss' | 'dividend';

interface InvestmentsTableProps {
  holdings: HoldingData[];
  onViewDetail: (holding: HoldingData) => void;
  onBuy: (holding: HoldingData) => void;
  onSell: (holding: HoldingData) => void;
  onUpdatePrice: (holding: HoldingData) => void;
  onDelete: (holding: HoldingData) => void;
  isLoading?: boolean;
  /** Tags per investment id; searched along with ticker and name and shown as badges. */
  tagsByInvestmentId?: ReadonlyMap<string, StockTag[]>;
}

/** Tag badges shown under the name before the rest collapse into "+N". */
const MAX_VISIBLE_TAGS = 3;
const EMPTY_TAGS: StockTag[] = [];

/**
 * Positions table (design system §6 Table, §7 List): E3 card with the search
 * and the zero-positions toggle in its head, sortable 10 px uppercase head on
 * `well`, 62 px rows with the instrument mark and ticker · currency, numbers
 * right and tabular, "···" actions on hover, footer with count, total and the
 * largest position's share.
 */
export function InvestmentsTable({
  holdings,
  onViewDetail,
  onBuy,
  onSell,
  onUpdatePrice,
  onDelete,
  isLoading,
  tagsByInvestmentId,
}: InvestmentsTableProps) {
  const { formatCurrency, formatCurrencyRaw, currencyCode } = useCurrency();
  const fmt = useFormat();
  const { t } = useTranslation('stocks');
  const [search, setSearch] = useState('');
  const [showZero, setShowZero] = useState(false);
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
    let rows = showZero ? holdings : holdings.filter((h) => h.quantity > 0);
    if (term) {
      rows = rows.filter((h) =>
        matchesSearch(term, [
          h.ticker,
          h.companyName,
          ...(tagsByInvestmentId?.get(h.id) ?? EMPTY_TAGS).map((tag) => tag.name),
        ])
      );
    }
    return [...rows].sort((a, b) => {
      let c = 0;
      switch (sortColumn) {
        case 'name':
          c = (a.companyName || '').localeCompare(b.companyName || '', undefined, {
            sensitivity: 'base',
          });
          break;
        case 'quantity':
          c = a.quantity - b.quantity;
          break;
        case 'avgCost':
          c = a.avgCost - b.avgCost;
          break;
        case 'currentPrice':
          c = a.currentPrice - b.currentPrice;
          break;
        case 'marketValue':
          c = a.marketValue - b.marketValue;
          break;
        case 'gainLoss':
          c = a.gainLoss - b.gainLoss;
          break;
        case 'dividend':
          c = (a.dividendYield || 0) * a.quantity - (b.dividendYield || 0) * b.quantity;
          break;
      }
      return sortDirection === 'asc' ? c : -c;
    });
  }, [holdings, search, showZero, sortColumn, sortDirection, tagsByInvestmentId]);

  const total = filtered.reduce((sum, h) => sum + h.marketValue, 0);
  const largest = filtered.reduce((max, h) => Math.max(max, h.marketValue), 0);
  const largestShare = total > 0 ? largest / total : 0;

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
        <CardTitle>{t('table.title')}</CardTitle>
        <div className="flex items-center gap-4">
          <label className="inline-flex cursor-pointer items-center gap-[9px] text-table font-500 text-ink-2">
            <Checkbox checked={showZero} onCheckedChange={(v) => setShowZero(v === true)} />
            {t('table.showZeroPositions')}
          </label>
          <InputWrap icon={<Search />} className="w-60">
            <Input
              className="h-[35px] text-table"
              placeholder={t('table.search')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              data-testid="input-search-instrument"
            />
          </InputWrap>
          <Button asChild variant="ghost" size="sm">
            <Link href="/reports/stocks-analysis">
              <Tag />
              {t('table.manageTags')}
            </Link>
          </Button>
        </div>
      </CardHeader>

      <Table>
        <TableHeader>
          <TableRow>
            <SortHead column="name" align="left" className="w-[26%]">
              {t('table.investment')}
            </SortHead>
            <SortHead column="quantity">{t('table.quantity')}</SortHead>
            <SortHead column="avgCost">{t('table.avgCost')}</SortHead>
            <SortHead column="currentPrice">{t('table.marketPrice')}</SortHead>
            <SortHead column="marketValue">{t('table.marketValue')}</SortHead>
            <SortHead column="gainLoss">{t('table.return')}</SortHead>
            <SortHead column="dividend">{t('table.dividend')}</SortHead>
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
                    {showZero ? t('table.noResults.description') : t('table.noResults.zeroHidden')}
                  </p>
                </div>
              </TableCell>
            </TableRow>
          ) : (
            filtered.map((h) => {
              const zero = h.quantity <= 0;
              const dividend = h.dividendYield ? h.dividendYield * h.quantity : 0;
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
                        type="stock"
                        variant={zero ? 'soft' : 'series'}
                      />
                      <div className="min-w-0">
                        <b className="block truncate text-table font-650">{h.companyName}</b>
                        <small className="mt-[3px] block text-micro font-500 text-ink-4">
                          {h.ticker} · {h.originalCurrency ?? currencyCode}
                        </small>
                        <HoldingTags tags={tagsByInvestmentId?.get(h.id) ?? EMPTY_TAGS} />
                      </div>
                    </div>
                  </TableCell>
                  <TableCell className="text-right num">
                    {fmt.number(h.quantity, { maximumFractionDigits: 4 })} {t('table.unit')}
                  </TableCell>
                  <TableCell className="text-right num">
                    {price(h.avgCost, currencyCode)}
                    {h.originalAvgCostCurrency && h.originalAvgCostCurrency !== currencyCode && (
                      <small className="mt-[3px] block text-micro font-500 text-ink-4">
                        {price(h.originalAvgCost || 0, h.originalAvgCostCurrency)}
                      </small>
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
                          <TooltipContent>{t('tooltips.manualPrice')}</TooltipContent>
                        </Tooltip>
                      )}
                      {price(h.currentPrice, currencyCode)}
                    </span>
                    {h.originalCurrency && h.originalCurrency !== currencyCode && (
                      <small className="mt-[3px] block text-micro font-500 text-ink-4">
                        {price(h.originalCurrentPrice || 0, h.originalCurrency)}
                      </small>
                    )}
                  </TableCell>
                  <TableCell className="text-right font-650 text-ink num">
                    {zero ? (
                      <span className="text-ink-4">—</span>
                    ) : (
                      formatCurrencyRaw(h.marketValue)
                    )}
                  </TableCell>
                  <TableCell className="text-right num">
                    {zero ? (
                      <span className="text-ink-4">
                        {h.realizedGain
                          ? t('table.realized', {
                              amount: fmt.money(h.realizedGain, currencyCode, {
                                signed: true,
                                decimals: 0,
                              }),
                            })
                          : '—'}
                      </span>
                    ) : (
                      <span
                        className={cn(
                          'block font-650',
                          h.gainLoss >= 0 ? 'text-gain' : 'text-loss'
                        )}
                      >
                        {fmt.percent(h.gainLossPercent / 100, 1, { signed: true })}
                        <small className="mt-[3px] block text-micro font-500 text-ink-4">
                          {fmt.money(h.gainLoss, currencyCode, { signed: true, decimals: 0 })}
                        </small>
                      </span>
                    )}
                  </TableCell>
                  <TableCell className="text-right num">
                    <span className="inline-flex items-center justify-end gap-1.5">
                      {h.isManualDividend && (
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Badge variant="outline" className="cursor-help">
                              {t('badges.manual')}
                            </Badge>
                          </TooltipTrigger>
                          <TooltipContent>{t('tooltips.manualDividend')}</TooltipContent>
                        </Tooltip>
                      )}
                      {dividend > 0 ? (
                        formatCurrency(dividend)
                      ) : (
                        <span className="text-ink-4">—</span>
                      )}
                    </span>
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
                          <DropdownMenuItem onSelect={() => onSell(h)} disabled={zero}>
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
          {t('table.footer.count', { count: filtered.length })} · {t('table.footer.total')}{' '}
          <span className="num">{formatCurrencyRaw(total)}</span>
          {filtered.length > 1 && largestShare > 0 && (
            <> · {t('table.footer.largest', { percent: fmt.percent(largestShare, 0) })}</>
          )}
        </span>
        <span>{t('table.footer.rates', { currency: currencyCode })}</span>
      </div>
    </Card>
  );
}

/** The holding's tags as small badges under the ticker; the rest collapse into "+N". */
function HoldingTags({ tags }: { tags: StockTag[] }) {
  if (tags.length === 0) return null;
  const visible = tags.slice(0, MAX_VISIBLE_TAGS);
  const hidden = tags.slice(MAX_VISIBLE_TAGS);
  return (
    <div className="mt-1 flex flex-wrap gap-1">
      {visible.map((tag) => (
        <Badge key={tag.id}>{tag.name}</Badge>
      ))}
      {hidden.length > 0 && (
        <Tooltip>
          <TooltipTrigger asChild>
            <Badge variant="outline" className="cursor-help">
              +{hidden.length}
            </Badge>
          </TooltipTrigger>
          <TooltipContent>{hidden.map((tag) => tag.name).join(', ')}</TooltipContent>
        </Tooltip>
      )}
    </div>
  );
}
