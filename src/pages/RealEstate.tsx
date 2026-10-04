import { useMemo, useState, type ReactNode } from 'react';
import { Link, useLocation } from 'wouter';
import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Ellipsis,
  ExternalLink,
  House,
  Pencil,
  Plus,
  Search,
  Trash2,
} from 'lucide-react';
import type { Loan, RealEstate } from '@shared/schema';
import { convertToCzK, type CurrencyCode } from '@shared/currencies';
import { exportApi, portfolioApi, realEstateApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useAuth } from '@/hooks/use-auth';
import { translateApiError } from '@/lib/translate-api-error';
import { useShellPage } from '@/components/shell/shell-context';
import { PageHead } from '@/components/shell/PageHead';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
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
import { AssetLogo } from '@/components/common/AssetLogo';
import { EmptyState } from '@/components/common/EmptyState';
import { ExportButton } from '@/components/common/ExportButton';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import PortfolioTrendCard from '@/components/common/PortfolioTrendCard';
import { AddRealEstateModal } from '@/components/real-estate/AddRealEstateModal';
import { RevalueDialog } from '@/components/valuations/RevalueDialog';
import { cn } from '@/lib/utils';

/** A property with the list's derived numbers, all in CZK. */
interface PropertyRow {
  property: RealEstate;
  valueCzk: number;
  purchaseCzk: number;
  appreciation: number;
  rentYearCzk: number;
  grossYield: number;
  loansCzk: number;
  equityCzk: number;
  ltv: number;
  personal: boolean;
  /** Personal residence left out of net worth by the profile setting. */
  excluded: boolean;
}

type SortColumn = 'name' | 'purchase' | 'value' | 'appreciation' | 'rent' | 'equity';
type Modal = 'edit' | 'revalue' | 'delete' | null;

const czk = (amount: string | number | null | undefined, currency: string | null | undefined) =>
  convertToCzK(Number(amount) || 0, (currency || 'CZK') as CurrencyCode);

/**
 * Real estate list (design system §7 List, prototype real-estate.html): four
 * stats, the value trend, and a table with appreciation, rental yield and
 * equity / LTV per property; the personal residence is marked when it sits
 * outside net worth.
 */
export default function RealEstatePage() {
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const { formatCurrency, formatCurrencySigned } = useCurrency();
  const fmt = useFormat();
  const { user } = useAuth();
  const excludePersonal = user?.excludePersonalRealEstate ?? false;
  const [search, setSearch] = useState('');
  const [sortColumn, setSortColumn] = useState<SortColumn>('value');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('desc');
  const [modal, setModal] = useState<Modal>(null);
  const [selected, setSelected] = useState<RealEstate | null>(null);

  const { data: properties = [], isLoading } = useQuery<RealEstate[]>({
    queryKey: ['real-estate'],
    queryFn: () => realEstateApi.getAll(),
  });

  // Linked loans decide the equity of each property (amortized balance)
  const loanQueries = useQueries({
    queries: properties.map((p) => ({
      queryKey: ['real-estate-loans', p.id],
      queryFn: () => realEstateApi.getLoans(p.id),
      staleTime: 60 * 1000,
    })),
  });

  const rows = useMemo((): PropertyRow[] => {
    return properties.map((p, i) => {
      const loans: Loan[] = loanQueries[i]?.data ?? [];
      const valueCzk = czk(p.marketPrice, p.marketPriceCurrency);
      const purchaseCzk = czk(p.purchasePrice, p.purchasePriceCurrency);
      const rentYearCzk = czk(Number(p.monthlyRent || 0) * 12, p.monthlyRentCurrency);
      const loansCzk = loans.reduce((s, l) => s + czk(l.outstandingBalance, l.currency), 0);
      const personal = p.type === 'personal';
      return {
        property: p,
        valueCzk,
        purchaseCzk,
        appreciation: valueCzk - purchaseCzk,
        rentYearCzk,
        grossYield: valueCzk > 0 ? rentYearCzk / valueCzk : 0,
        loansCzk,
        equityCzk: valueCzk - loansCzk,
        ltv: valueCzk > 0 ? loansCzk / valueCzk : 0,
        personal,
        excluded: personal && excludePersonal,
      };
    });
    // loanQueries is a new array every render; its data is what matters
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [properties, excludePersonal, ...loanQueries.map((q) => q.data)]);

  const counted = rows.filter((r) => !r.excluded);
  const totals = counted.reduce(
    (acc, r) => ({
      value: acc.value + r.valueCzk,
      purchase: acc.purchase + r.purchaseCzk,
      rent: acc.rent + r.rentYearCzk,
      rented: acc.rented + (r.rentYearCzk > 0 ? r.valueCzk : 0),
      loans: acc.loans + r.loansCzk,
    }),
    { value: 0, purchase: 0, rent: 0, rented: 0, loans: 0 }
  );
  const personalValue = rows.filter((r) => r.personal).reduce((s, r) => s + r.valueCzk, 0);
  const latest = properties.reduce<RealEstate | null>(
    (max, p) => (max === null || p.updatedAt > max.updatedAt ? p : max),
    null
  );

  useShellPage({
    status: latest
      ? {
          text: t('status.lastRevaluation', { date: fmt.day(latest.updatedAt), name: latest.name }),
          tone: 'neutral',
        }
      : { text: t('status.none'), tone: 'neutral' },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => realEstateApi.delete(id),
    onSuccess: async () => {
      queryClient.invalidateQueries({ queryKey: ['real-estate'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      try {
        await portfolioApi.recordSnapshot();
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      } catch (error) {
        console.error('Failed to record portfolio snapshot:', error);
      }
      toast(t('toast.deleted'));
      setModal(null);
      setSelected(null);
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  const handleSort = (column: SortColumn) => {
    if (sortColumn === column) setSortDirection((d) => (d === 'asc' ? 'desc' : 'asc'));
    else {
      setSortColumn(column);
      setSortDirection(column === 'name' ? 'asc' : 'desc');
    }
  };

  const visible = useMemo(() => {
    const term = search.trim().toLowerCase();
    const list = term
      ? rows.filter((r) => `${r.property.name} ${r.property.address}`.toLowerCase().includes(term))
      : rows;
    return [...list].sort((a, b) => {
      let c = 0;
      switch (sortColumn) {
        case 'name':
          c = a.property.name.localeCompare(b.property.name, undefined, { sensitivity: 'base' });
          break;
        case 'purchase':
          c = a.purchaseCzk - b.purchaseCzk;
          break;
        case 'value':
          c = a.valueCzk - b.valueCzk;
          break;
        case 'appreciation':
          c = a.appreciation - b.appreciation;
          break;
        case 'rent':
          c = a.rentYearCzk - b.rentYearCzk;
          break;
        case 'equity':
          c = a.equityCzk - b.equityCzk;
          break;
      }
      return sortDirection === 'asc' ? c : -c;
    });
  }, [rows, search, sortColumn, sortDirection]);

  const openModal = (which: Modal, property: RealEstate) => {
    setSelected(property);
    setModal(which);
  };
  const closeModal = () => {
    setModal(null);
    setSelected(null);
  };

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

  const isEmpty = !isLoading && properties.length === 0;
  const sub = (text: string) => (
    <small className="mt-[3px] block text-micro font-500 text-ink-4">{text}</small>
  );

  return (
    <>
      <PageHead
        eyebrow={t('eyebrow')}
        title={t('title')}
        description={t('subtitle')}
        actions={
          !isEmpty && (
            <>
              <ExportButton exportFn={exportApi.realEstate} label={t('export')} />
              <AddRealEstateModal
                trigger={
                  <Button>
                    <Plus />
                    {t('addProperty')}
                  </Button>
                }
              />
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
          icon={<House />}
          title={t('empty.title')}
          description={t('empty.description')}
          action={
            <AddRealEstateModal
              trigger={
                <Button>
                  <Plus />
                  {t('addProperty')}
                </Button>
              }
            />
          }
        />
      ) : (
        <>
          <Stats>
            <Stat
              label={t('summary.marketValue')}
              value={formatCurrency(totals.value)}
              note={[
                t('summary.properties', { count: counted.length }),
                personalValue > 0
                  ? excludePersonal
                    ? t('summary.excludedNote', { amount: formatCurrency(personalValue) })
                    : t('summary.includedNote', { amount: formatCurrency(personalValue) })
                  : null,
              ]
                .filter(Boolean)
                .join(' · ')}
            />
            <Stat
              label={t('summary.appreciationTitle')}
              value={formatCurrencySigned(totals.value - totals.purchase)}
              tone={totals.value - totals.purchase >= 0 ? 'gain' : 'loss'}
              note={`${fmt.percent(totals.purchase > 0 ? (totals.value - totals.purchase) / totals.purchase : 0, 1, { signed: true })} ${t('summary.vsPurchase', { amount: formatCurrency(totals.purchase) })}`}
            />
            <Stat
              label={t('summary.equity')}
              value={formatCurrency(totals.value - totals.loans)}
              note={
                totals.loans > 0
                  ? t('summary.equityNote', {
                      amount: formatCurrency(totals.loans),
                      ltv: fmt.percent(totals.value > 0 ? totals.loans / totals.value : 0, 0),
                    })
                  : t('summary.noLoans')
              }
            />
            <Stat
              label={t('summary.grossRentYear')}
              value={formatCurrencySigned(totals.rent)}
              tone={totals.rent > 0 ? 'gain' : 'neutral'}
              noteTone="neutral"
              note={
                totals.rent > 0
                  ? t('summary.rentNote', {
                      monthly: formatCurrency(totals.rent / 12),
                      yield: fmt.percent(totals.rented > 0 ? totals.rent / totals.rented : 0, 1),
                    })
                  : t('summary.noRent')
              }
            />
          </Stats>

          <PortfolioTrendCard
            type="realEstate"
            currentValue={totals.value + (excludePersonal ? personalValue : 0)}
          />

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
                    {t('table.property')}
                  </SortHead>
                  <TableHead>{t('table.type')}</TableHead>
                  <SortHead column="purchase">{t('table.purchasePrice')}</SortHead>
                  <SortHead column="value">{t('table.marketValue')}</SortHead>
                  <SortHead column="appreciation">{t('table.appreciation')}</SortHead>
                  <SortHead column="rent">{t('table.rentYield')}</SortHead>
                  <SortHead column="equity">{t('table.equity')}</SortHead>
                  <TableHead className="w-10">
                    <span className="sr-only">{t('table.actions')}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {visible.length === 0 ? (
                  <TableRow className="hover:bg-transparent">
                    <TableCell colSpan={8} className="py-12">
                      <div className="grid place-items-center text-center text-ink-3">
                        <Search className="mb-3 size-7 text-ink-5" aria-hidden />
                        <h3 className="mb-1.5 text-h3 text-ink">{t('table.noResults.title')}</h3>
                        <p className="m-0 max-w-[320px] text-table">
                          {t('table.noResults.description')}
                        </p>
                      </div>
                    </TableCell>
                  </TableRow>
                ) : (
                  visible.map((r) => {
                    const p = r.property;
                    return (
                      <TableRow
                        key={p.id}
                        className={cn('h-[66px] cursor-pointer', r.excluded && 'text-ink-4')}
                        onClick={() => setLocation(`/real-estate/${p.id}`)}
                        tabIndex={0}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') setLocation(`/real-estate/${p.id}`);
                        }}
                      >
                        <TableCell>
                          <div className="flex min-w-0 items-center gap-[10px]">
                            <AssetLogo
                              ticker={p.name}
                              type="stock"
                              variant={r.excluded ? 'soft' : 'series'}
                            />
                            <div className="min-w-0">
                              <b
                                className={cn(
                                  'block truncate text-table font-650',
                                  r.excluded ? 'text-ink-3' : 'text-ink'
                                )}
                              >
                                {p.name}
                              </b>
                              <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">
                                {p.address}
                              </small>
                            </div>
                          </div>
                        </TableCell>
                        <TableCell>
                          {r.excluded ? (
                            <Badge variant="outline">{t('table.excluded')}</Badge>
                          ) : (
                            <Badge>{t(`types.${p.type}`)}</Badge>
                          )}
                        </TableCell>
                        <TableCell className="text-right num">
                          {formatCurrency(r.purchaseCzk)}
                        </TableCell>
                        <TableCell
                          className={cn(
                            'text-right font-650 num',
                            r.excluded ? 'text-ink-3' : 'text-ink'
                          )}
                        >
                          {formatCurrency(r.valueCzk)}
                        </TableCell>
                        <TableCell
                          className={cn(
                            'text-right num',
                            !r.excluded && (r.appreciation >= 0 ? 'text-gain' : 'text-loss')
                          )}
                        >
                          <span className="font-650">{formatCurrencySigned(r.appreciation)}</span>
                          {sub(
                            fmt.percent(r.purchaseCzk > 0 ? r.appreciation / r.purchaseCzk : 0, 1, {
                              signed: true,
                            })
                          )}
                        </TableCell>
                        <TableCell className="text-right num">
                          {r.rentYearCzk > 0 ? (
                            <>
                              {formatCurrency(r.rentYearCzk / 12)}
                              {sub(
                                t('table.monthlyYield', { yield: fmt.percent(r.grossYield, 1) })
                              )}
                            </>
                          ) : (
                            <span className="text-ink-4">{t('table.noRent')}</span>
                          )}
                        </TableCell>
                        <TableCell className="text-right num">
                          {r.loansCzk > 0 ? (
                            <>
                              <span className="inline-flex items-center justify-end gap-2">
                                <Progress value={(1 - r.ltv) * 100} className="w-[54px]" />
                                <span className="font-650 text-ink">
                                  {formatCurrency(r.equityCzk)}
                                </span>
                              </span>
                              {sub(
                                t('table.loanNote', {
                                  amount: formatCurrency(r.loansCzk),
                                  ltv: fmt.percent(r.ltv, 0),
                                })
                              )}
                            </>
                          ) : (
                            <>
                              <span
                                className={cn('font-650', r.excluded ? 'text-ink-3' : 'text-ink')}
                              >
                                {formatCurrency(r.equityCzk)}
                              </span>
                              {sub(t('table.noLoan'))}
                            </>
                          )}
                        </TableCell>
                        <TableCell className="text-right">
                          <div data-row-actions onClick={(e) => e.stopPropagation()}>
                            <DropdownMenu>
                              <DropdownMenuTrigger asChild>
                                <Button
                                  variant="ghost"
                                  size="icon-sm"
                                  aria-label={t('table.actions')}
                                >
                                  <Ellipsis />
                                </Button>
                              </DropdownMenuTrigger>
                              <DropdownMenuContent align="end">
                                <DropdownMenuItem
                                  onSelect={() => setLocation(`/real-estate/${p.id}`)}
                                >
                                  <ExternalLink />
                                  {t('actions.open')}
                                </DropdownMenuItem>
                                <DropdownMenuItem onSelect={() => openModal('edit', p)}>
                                  <Pencil />
                                  {t('actions.edit')}
                                </DropdownMenuItem>
                                <DropdownMenuItem onSelect={() => openModal('revalue', p)}>
                                  <ArrowUp />
                                  {t('actions.revalue')}
                                </DropdownMenuItem>
                                <DropdownMenuSeparator />
                                <DropdownMenuItem
                                  variant="danger"
                                  onSelect={() => openModal('delete', p)}
                                >
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
                {t('table.footer.count', { count: counted.length })} ·{' '}
                {t('table.footer.value', { amount: formatCurrency(totals.value) })} ·{' '}
                {t('table.footer.equity', { amount: formatCurrency(totals.value - totals.loans) })}
              </span>
              {personalValue > 0 && (
                <span>
                  {excludePersonal
                    ? t('table.footer.personalExcluded')
                    : t('table.footer.personalIncluded')}{' '}
                  ·{' '}
                  <Link
                    href="/settings"
                    className="font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
                  >
                    {t('table.footer.change')}
                  </Link>
                </span>
              )}
            </div>
          </Card>
        </>
      )}

      {selected && (
        <AddRealEstateModal
          key={selected.id}
          realEstate={selected}
          open={modal === 'edit'}
          onOpenChange={(open) => {
            if (!open) closeModal();
          }}
        />
      )}
      <RevalueDialog
        open={modal === 'revalue'}
        onOpenChange={(open) => {
          if (!open) closeModal();
        }}
        kind="realEstate"
        asset={
          selected
            ? {
                id: selected.id,
                name: selected.name,
                currentValue: Number(selected.marketPrice) || 0,
                currency: selected.marketPriceCurrency || 'CZK',
              }
            : null
        }
      />
      <ConfirmDeleteDialog
        open={modal === 'delete'}
        onOpenChange={(open) => {
          if (!open) closeModal();
        }}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.description', { name: selected?.name ?? '' })}
        onConfirm={() => selected && deleteMutation.mutate(selected.id)}
        isPending={deleteMutation.isPending}
        confirmLabel={t('actions.delete')}
      />
    </>
  );
}
