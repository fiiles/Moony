import { useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowDown, ArrowUp, ArrowUpDown, Ellipsis, Pencil, Search, Trash2 } from 'lucide-react';
import type { Bond } from '@shared/schema';
import type { CurrencyCode } from '@shared/currencies';
import { bondCouponCzk, bondFaceValue, bondValueCzk, isMatured } from '@/utils/bonds';
import { monthsBetween } from '@/utils/duration';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
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
import { AssetLogo } from '@/components/common/AssetLogo';
import { untilText } from '@/components/bonds/bond-until';
import { cn } from '@/lib/utils';

type SortColumn = 'name' | 'nominal' | 'quantity' | 'value' | 'rate' | 'yearly' | 'maturity';

interface BondsTableProps {
  bonds: Bond[];
  today: number;
  onEdit: (bond: Bond) => void;
  onDelete: (bond: Bond) => void;
}

/**
 * Issues table (design system §6 Table, prototype bonds.html): nearest
 * maturity first, per-bond yearly coupon, "do roka" badge, matured issues
 * dimmed behind a toggle, "···" actions on hover.
 */
export function BondsTable({ bonds, today, onEdit, onDelete }: BondsTableProps) {
  const { formatCurrency, currencyCode } = useCurrency();
  const fmt = useFormat();
  const { t } = useTranslation('bonds');
  const [search, setSearch] = useState('');
  const [showMatured, setShowMatured] = useState(false);
  const [sortColumn, setSortColumn] = useState<SortColumn>('maturity');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('asc');

  const handleSort = (column: SortColumn) => {
    if (sortColumn === column) setSortDirection((d) => (d === 'asc' ? 'desc' : 'asc'));
    else {
      setSortColumn(column);
      setSortDirection(column === 'name' || column === 'maturity' ? 'asc' : 'desc');
    }
  };

  const maturedCount = bonds.filter((b) => isMatured(b, today)).length;

  const rows = useMemo(() => {
    const term = search.trim().toLowerCase();
    let list = showMatured ? bonds : bonds.filter((b) => !isMatured(b, today));
    if (term) {
      list = list.filter((b) => `${b.name} ${b.isin ?? ''}`.toLowerCase().includes(term));
    }
    return [...list].sort((a, b) => {
      let c = 0;
      switch (sortColumn) {
        case 'name':
          c = a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
          break;
        case 'nominal':
          c = (parseFloat(a.couponValue) || 0) - (parseFloat(b.couponValue) || 0);
          break;
        case 'quantity':
          c = (parseFloat(a.quantity) || 0) - (parseFloat(b.quantity) || 0);
          break;
        case 'value':
          c = bondValueCzk(a) - bondValueCzk(b);
          break;
        case 'rate':
          c = (parseFloat(a.interestRate) || 0) - (parseFloat(b.interestRate) || 0);
          break;
        case 'yearly':
          c = bondCouponCzk(a) - bondCouponCzk(b);
          break;
        case 'maturity':
          c =
            (a.maturityDate ?? Number.MAX_SAFE_INTEGER) -
            (b.maturityDate ?? Number.MAX_SAFE_INTEGER);
          break;
      }
      return sortDirection === 'asc' ? c : -c;
    });
  }, [bonds, search, showMatured, sortColumn, sortDirection, today]);

  const live = rows.filter((b) => !isMatured(b, today));
  const liveNominal = live.reduce((s, b) => s + bondValueCzk(b), 0);
  const liveCoupons = live.reduce((s, b) => s + bondCouponCzk(b), 0);

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

  return (
    <Card variant="table">
      <CardHeader>
        <div>
          <CardTitle>{t('table.title')}</CardTitle>
          <CardDescription>{t('table.subtitle')}</CardDescription>
        </div>
        <div className="flex items-center gap-4">
          {maturedCount > 0 && (
            <label className="inline-flex cursor-pointer items-center gap-[9px] text-table font-500 text-ink-2">
              <Checkbox checked={showMatured} onCheckedChange={(v) => setShowMatured(v === true)} />
              {t('table.showMatured')}
            </label>
          )}
          <InputWrap icon={<Search />} className="w-60">
            <Input
              className="h-[35px] text-table"
              placeholder={t('table.search')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </InputWrap>
        </div>
      </CardHeader>

      <Table>
        <TableHeader>
          <TableRow>
            <SortHead column="name" align="left" className="w-[28%]">
              {t('table.bond')}
            </SortHead>
            <SortHead column="nominal">{t('table.nominal')}</SortHead>
            <SortHead column="quantity">{t('table.quantity')}</SortHead>
            <SortHead column="value">{t('table.value')}</SortHead>
            <SortHead column="rate">{t('table.coupon')}</SortHead>
            <SortHead column="yearly">{t('table.yearly')}</SortHead>
            <SortHead column="maturity">{t('table.maturity')}</SortHead>
            <TableHead className="w-10">
              <span className="sr-only">{t('table.actions')}</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.length === 0 ? (
            <TableRow className="hover:bg-transparent">
              <TableCell colSpan={8} className="py-12">
                <div className="grid place-items-center text-center text-ink-3">
                  <Search className="mb-3 size-7 text-ink-5" aria-hidden />
                  <h3 className="mb-1.5 text-h3 text-ink">{t('table.noResults.title')}</h3>
                  <p className="m-0 max-w-[320px] text-table">
                    {showMatured || maturedCount === 0
                      ? t('table.noResults.description')
                      : t('table.noResults.maturedHidden')}
                  </p>
                </div>
              </TableCell>
            </TableRow>
          ) : (
            rows.map((bond) => {
              const matured = isMatured(bond, today);
              const currency = (bond.currency || 'CZK') as CurrencyCode;
              const nominal = parseFloat(bond.couponValue) || 0;
              const withinYear =
                !matured &&
                bond.maturityDate !== null &&
                monthsBetween(today, bond.maturityDate).totalMonths < 12;
              return (
                <TableRow key={bond.id} className={cn('h-[62px]', matured && 'text-ink-4')}>
                  <TableCell>
                    <div className="flex min-w-0 items-center gap-[10px]">
                      <AssetLogo
                        ticker={bond.isin || bond.name}
                        type="stock"
                        variant={matured ? 'soft' : 'series'}
                      />
                      <div className="min-w-0">
                        <b
                          className={cn(
                            'block truncate text-table font-650',
                            matured ? 'text-ink-3' : 'text-ink'
                          )}
                        >
                          {bond.name}
                        </b>
                        <small className="mt-[3px] block text-micro font-500 text-ink-4">
                          {bond.isin || '—'} · {currency}
                        </small>
                      </div>
                    </div>
                  </TableCell>
                  <TableCell className="text-right num">{fmt.money(nominal, currency)}</TableCell>
                  <TableCell className="text-right num">
                    {fmt.number(parseFloat(bond.quantity) || 0)} {t('table.units')}
                  </TableCell>
                  <TableCell
                    className={cn('text-right font-650 num', matured ? 'text-ink-3' : 'text-ink')}
                  >
                    {currency === currencyCode
                      ? fmt.money(bondFaceValue(bond), currency)
                      : formatCurrency(bondValueCzk(bond))}
                  </TableCell>
                  <TableCell className="text-right num">
                    {fmt.percent((parseFloat(bond.interestRate) || 0) / 100, 2)}
                  </TableCell>
                  <TableCell className="text-right num">
                    {matured ? (
                      <span className="text-ink-4">—</span>
                    ) : (
                      formatCurrency(bondCouponCzk(bond))
                    )}
                  </TableCell>
                  <TableCell className="text-right num">
                    {bond.maturityDate === null ? (
                      <span className="text-ink-4">{t('table.noMaturity')}</span>
                    ) : (
                      <>
                        <span className="inline-flex items-center justify-end gap-1.5">
                          {matured ? (
                            <Badge>{t('table.matured')}</Badge>
                          ) : (
                            withinYear && <Badge variant="dark">{t('table.withinYear')}</Badge>
                          )}
                          {fmt.day(bond.maturityDate)}
                        </span>
                        <small className="mt-[3px] block text-micro font-500 text-ink-4">
                          {untilText(t, bond.maturityDate, today, fmt.day)}
                        </small>
                      </>
                    )}
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
                          <DropdownMenuItem onSelect={() => onEdit(bond)}>
                            <Pencil />
                            {t('actions.edit')}
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem variant="danger" onSelect={() => onDelete(bond)}>
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
          {t('table.footer.count', { count: live.length })} ·{' '}
          {t('table.footer.nominal', { amount: formatCurrency(liveNominal) })} ·{' '}
          {t('table.footer.coupons', { amount: formatCurrency(liveCoupons) })}
        </span>
        <span>{t('table.footer.valuation')}</span>
      </div>
    </Card>
  );
}
