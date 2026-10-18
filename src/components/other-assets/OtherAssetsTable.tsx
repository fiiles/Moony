import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ArrowUp,
  Ellipsis,
  History,
  Pencil,
  Search,
  ShoppingCart,
  Trash2,
  TrendingDown,
} from 'lucide-react';
import type { OtherAsset } from '@shared/schema';
import { recordCurrency } from '@shared/currencies';
import type { OtherAssetRow } from '@/utils/other-assets';
import { compareConverted } from '@/utils/fx-totals';
import { ConvertedMoney } from '@/components/common/ConvertedMoney';
import { SortableTableHead } from '@/components/common/SortableTableHead';
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
import { ChangeBadge } from '@/components/common/ChangeBadge';

type SortColumn = 'name' | 'quantity' | 'price' | 'avgPurchase' | 'value' | 'gain' | 'yield';

interface OtherAssetsTableProps {
  rows: OtherAssetRow[];
  /** The purchase-day rates or the transactions are still loading: costs are not known yet. */
  costPending?: boolean;
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
  costPending = false,
  onViewTransactions,
  onBuy,
  onSell,
  onEdit,
  onRevalue,
  onDelete,
}: OtherAssetsTableProps) {
  const { t } = useTranslation('otherAssets');
  const { currencyCode } = useCurrency();
  const fmt = useFormat();
  const money = (value: number) => fmt.money(value, currencyCode, { decimals: 0 });
  const signed = { signed: true, decimals: 0 };
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
          c = compareConverted(a.value, b.value);
          break;
        case 'gain':
          c = compareConverted(a.gain, b.gain);
          break;
        case 'yield':
          c = compareConverted(a.yield, b.yield);
          break;
      }
      return sortDirection === 'asc' ? c : -c;
    });
  }, [rows, search, sortColumn, sortDirection]);

  // Footer totals: amounts without a rate stay out (the page note names them)
  const totalValue = visible.reduce((s, r) => s + (r.value ?? 0), 0);
  const totalCost = visible.reduce((s, r) => s + (r.cost ?? 0), 0);

  const sortProps = { sortColumn, sortDirection, onSort: handleSort };

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
            <SortableTableHead {...sortProps} column="name" align="left" className="w-[24%]">
              {t('table.asset')}
            </SortableTableHead>
            <SortableTableHead {...sortProps} column="quantity">
              {t('table.quantity')}
            </SortableTableHead>
            <SortableTableHead {...sortProps} column="price">
              {t('table.price')}
            </SortableTableHead>
            <SortableTableHead {...sortProps} column="avgPurchase">
              {t('table.avgPurchase')}
            </SortableTableHead>
            <SortableTableHead {...sortProps} column="value">
              {t('table.value')}
            </SortableTableHead>
            <SortableTableHead {...sortProps} column="gain">
              {t('table.gain')}
            </SortableTableHead>
            <SortableTableHead {...sortProps} column="yield">
              {t('table.yearly')}
            </SortableTableHead>
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
              const currency = recordCurrency(a.currency);
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
                    {fmt.money(r.price, r.priceCurrency, { decimals: 0 })}
                  </TableCell>
                  <TableCell className="text-right num">
                    {r.avgPurchase > 0 ? (
                      fmt.money(r.avgPurchase, currency, { decimals: 0 })
                    ) : (
                      <span className="text-ink-4">—</span>
                    )}
                  </TableCell>
                  <TableCell className="text-right font-650 text-ink num">
                    <ConvertedMoney
                      value={r.value}
                      amount={r.valueNative}
                      currency={r.priceCurrency}
                    />
                  </TableCell>
                  <TableCell className="text-right num text-ink-4">
                    {r.costNative === 0 || r.gain === 0 ? (
                      t('table.noChange')
                    ) : r.gain === null && r.gainNative === null ? (
                      // Price and purchase average in different currencies: no native gain
                      '—'
                    ) : r.gain === null ? (
                      // Native gain until the purchase days convert (or with the missing marker)
                      <ConvertedMoney
                        className="font-650"
                        value={null}
                        amount={r.gainNative ?? 0}
                        currency={currency}
                        options={signed}
                        pending={costPending}
                      />
                    ) : (
                      <>
                        <ChangeBadge ratio={r.gainPct ?? 0} />
                        {sub(fmt.money(r.gain, currencyCode, signed))}
                      </>
                    )}
                  </TableCell>
                  <TableCell className="text-right num">
                    {r.yieldNative > 0 ? (
                      <>
                        <ConvertedMoney
                          className="font-650 text-gain"
                          value={r.yield}
                          amount={r.yieldNative}
                          currency={r.yieldCurrency}
                          options={signed}
                        />
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
          {t('table.footer.value', { amount: money(totalValue) })} ·{' '}
          {t('table.footer.cost', { amount: money(totalCost) })}
        </span>
        <span>{t('table.footer.yieldNote')}</span>
      </div>
    </Card>
  );
}
