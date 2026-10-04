import { useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Ellipsis,
  History,
  Pencil,
  Search,
  ShoppingCart,
  Trash2,
  TrendingDown,
} from 'lucide-react';
import type { OtherAsset } from '@shared/schema';
import type { OtherAssetRow } from '@/utils/other-assets';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
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
import { AssetLogo } from '@/components/common/AssetLogo';
import { cn } from '@/lib/utils';

type SortColumn = 'name' | 'quantity' | 'price' | 'avgPurchase' | 'value' | 'gain' | 'yield';

interface OtherAssetsTableProps {
  rows: OtherAssetRow[];
  onViewTransactions: (asset: OtherAsset) => void;
  onBuy: (asset: OtherAsset) => void;
  onSell: (asset: OtherAsset) => void;
  onEdit: (asset: OtherAsset) => void;
  onRevalue: (asset: OtherAsset) => void;
  onDelete: (asset: OtherAsset) => void;
}

/**
 * Assets table (prototype other-assets.html): value from quantity × your
 * latest estimate, gain against the purchase average, the yearly yield with
 * its type, and the day of the last change; "···" offers transactions, buy,
 * sell, edit (restored), revalue and delete.
 */
export function OtherAssetsTable({
  rows,
  onViewTransactions,
  onBuy,
  onSell,
  onEdit,
  onRevalue,
  onDelete,
}: OtherAssetsTableProps) {
  const { t } = useTranslation('otherAssets');
  const { formatCurrency, formatCurrencySigned } = useCurrency();
  const fmt = useFormat();
  const [search, setSearch] = useState('');
  const [sortColumn, setSortColumn] = useState<SortColumn>('value');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('desc');

  const handleSort = (column: SortColumn) => {
    if (sortColumn === column) setSortDirection((d) => (d === 'asc' ? 'desc' : 'asc'));
    else {
      setSortColumn(column);
      setSortDirection(column === 'name' ? 'asc' : 'desc');
    }
  };

  const visible = useMemo(() => {
    const term = search.trim().toLowerCase();
    const list = term ? rows.filter((r) => r.asset.name.toLowerCase().includes(term)) : rows;
    return [...list].sort((a, b) => {
      let c = 0;
      switch (sortColumn) {
        case 'name':
          c = a.asset.name.localeCompare(b.asset.name, undefined, { sensitivity: 'base' });
          break;
        case 'quantity':
          c = a.quantity - b.quantity;
          break;
        case 'price':
          c = a.price - b.price;
          break;
        case 'avgPurchase':
          c = a.avgPurchase - b.avgPurchase;
          break;
        case 'value':
          c = a.valueCzk - b.valueCzk;
          break;
        case 'gain':
          c = a.gainCzk - b.gainCzk;
          break;
        case 'yield':
          c = a.yieldCzk - b.yieldCzk;
          break;
      }
      return sortDirection === 'asc' ? c : -c;
    });
  }, [rows, search, sortColumn, sortDirection]);

  const totalValue = visible.reduce((s, r) => s + r.valueCzk, 0);
  const totalCost = visible.reduce((s, r) => s + r.costCzk, 0);

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

  const sub = (text: string) => (
    <small className="mt-[3px] block text-micro font-500 text-ink-4">{text}</small>
  );
  const yieldLabel = (asset: OtherAsset) => {
    const value = fmt.number(parseFloat(asset.yieldValue || '0') || 0, {
      maximumFractionDigits: 2,
    });
    switch (asset.yieldType) {
      case 'fixed':
        return t('table.yieldFixed');
      case 'percent_purchase':
        return t('table.yieldPercentPurchase', { value });
      case 'percent_market':
        return t('table.yieldPercentMarket', { value });
      default:
        return '';
    }
  };

  return (
    <Card variant="table">
      <CardHeader>
        <div>
          <CardTitle>{t('table.title')}</CardTitle>
          <CardDescription>{t('table.subtitle')}</CardDescription>
        </div>
        <InputWrap icon={<Search />} className="w-60">
          <Input
            className="h-[35px] text-table"
            placeholder={t('table.search')}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </InputWrap>
      </CardHeader>
      <Table>
        <TableHeader>
          <TableRow>
            <SortHead column="name" align="left" className="w-[24%]">
              {t('table.asset')}
            </SortHead>
            <SortHead column="quantity">{t('table.quantity')}</SortHead>
            <SortHead column="price">{t('table.price')}</SortHead>
            <SortHead column="avgPurchase">{t('table.avgPurchase')}</SortHead>
            <SortHead column="value">{t('table.value')}</SortHead>
            <SortHead column="gain">{t('table.gain')}</SortHead>
            <SortHead column="yield">{t('table.yearly')}</SortHead>
            <TableHead className="text-right">{t('table.updated')}</TableHead>
            <TableHead className="w-10">
              <span className="sr-only">{t('table.actions')}</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {visible.length === 0 ? (
            <TableRow className="hover:bg-transparent">
              <TableCell colSpan={9} className="py-12">
                <div className="grid place-items-center text-center text-ink-3">
                  <Search className="mb-3 size-7 text-ink-5" aria-hidden />
                  <h3 className="mb-1.5 text-h3 text-ink">{t('table.noResults.title')}</h3>
                  <p className="m-0 max-w-[320px] text-table">{t('table.noResults.description')}</p>
                </div>
              </TableCell>
            </TableRow>
          ) : (
            visible.map((r) => {
              const a = r.asset;
              const currency = a.currency || 'CZK';
              return (
                <TableRow key={a.id} className="h-[62px]">
                  <TableCell>
                    <div className="flex min-w-0 items-center gap-[10px] text-ink">
                      <AssetLogo ticker={a.name} type="stock" />
                      <div className="min-w-0">
                        <b className="block truncate text-table font-650">{a.name}</b>
                        {sub(currency)}
                      </div>
                    </div>
                  </TableCell>
                  <TableCell className="text-right num">
                    {fmt.number(r.quantity, { maximumFractionDigits: 4 })}
                  </TableCell>
                  <TableCell className="text-right num">
                    {fmt.money(r.price, currency, { decimals: 0 })}
                  </TableCell>
                  <TableCell className="text-right num">
                    {r.avgPurchase > 0 ? (
                      fmt.money(r.avgPurchase, currency, { decimals: 0 })
                    ) : (
                      <span className="text-ink-4">—</span>
                    )}
                  </TableCell>
                  <TableCell className="text-right font-650 text-ink num">
                    {formatCurrency(r.valueCzk)}
                  </TableCell>
                  <TableCell
                    className={cn(
                      'text-right num',
                      r.gainCzk > 0 ? 'text-gain' : r.gainCzk < 0 ? 'text-loss' : 'text-ink-4'
                    )}
                  >
                    {r.costCzk === 0 || r.gainCzk === 0 ? (
                      t('table.noChange')
                    ) : (
                      <>
                        <span className="font-650">{formatCurrencySigned(r.gainCzk)}</span>
                        {sub(fmt.percent(r.gainPct, 1, { signed: true }))}
                      </>
                    )}
                  </TableCell>
                  <TableCell className="text-right num">
                    {r.yieldCzk > 0 ? (
                      <>
                        <span className="font-650 text-gain">
                          {formatCurrencySigned(r.yieldCzk)}
                        </span>
                        {sub(yieldLabel(a))}
                      </>
                    ) : (
                      <span className="text-ink-4">—</span>
                    )}
                  </TableCell>
                  <TableCell className="text-right num text-ink-3">
                    {fmt.day(a.updatedAt)}
                  </TableCell>
                  <TableCell className="text-right">
                    <div data-row-actions>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon-sm" aria-label={t('table.actions')}>
                            <Ellipsis />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem onSelect={() => onViewTransactions(a)}>
                            <History />
                            {t('actions.transactions')}
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => onBuy(a)}>
                            <ShoppingCart />
                            {t('actions.buy')}
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => onSell(a)} disabled={r.quantity <= 0}>
                            <TrendingDown />
                            {t('actions.sell')}
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem onSelect={() => onEdit(a)}>
                            <Pencil />
                            {t('actions.edit')}
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => onRevalue(a)}>
                            <ArrowUp />
                            {t('actions.revalue')}
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem variant="danger" onSelect={() => onDelete(a)}>
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
          {t('table.footer.count', { count: visible.length })} ·{' '}
          {t('table.footer.value', { amount: formatCurrency(totalValue) })} ·{' '}
          {t('table.footer.cost', { amount: formatCurrency(totalCost) })}
        </span>
        <span>{t('table.footer.yieldNote')}</span>
      </div>
    </Card>
  );
}
