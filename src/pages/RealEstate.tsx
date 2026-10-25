import { useMemo, useState } from 'react';
import { Link, useLocation } from 'wouter';
import {
  useMutation,
  useQueries,
  useQuery,
  useQueryClient,
  type UseQueryResult,
} from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { ArrowUp, Ellipsis, ExternalLink, House, Pencil, Plus, Search, Trash2 } from 'lucide-react';
import type { Loan, RealEstate } from '@shared/schema';
import { recordCurrency } from '@shared/currencies';
import { exportApi, portfolioApi, realEstateApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useAuth } from '@/hooks/use-auth';
import { useDatedMainConvert, useMainConvert } from '@/hooks/use-fx';
import {
  ALL_REAL_ESTATE_VALUATIONS_KEY,
  useAllRealEstateValuations,
} from '@/hooks/use-real-estate';
import { propertiesSummary, propertyRow, type PropertyRow } from '@/utils/real-estate';
import { compareConverted } from '@/utils/fx-totals';
import type { EventCluster } from '@/utils/chart-scale';
import {
  firstRealEstateDay,
  realEstateEvents,
  type RealEstateEvent,
} from '@/utils/real-estate-events';
import { ConvertedMoney } from '@/components/common/ConvertedMoney';
import { SortableTableHead } from '@/components/common/SortableTableHead';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';
import { translateApiError } from '@/lib/translate-api-error';
import { useShellPage } from '@/components/shell/shell-context';
import { PageHead } from '@/components/shell/PageHead';
import { Badge } from '@/components/ui/badge';
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
import { EmptyState } from '@/components/common/EmptyState';
import { McpTip } from '@/components/common/McpTip';
import { ExportButton } from '@/components/common/ExportButton';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import PortfolioTrendCard from '@/components/common/PortfolioTrendCard';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { AddRealEstateModal } from '@/components/real-estate/AddRealEstateModal';
import { RevalueDialog } from '@/components/valuations/RevalueDialog';
import { cn } from '@/lib/utils';
import { ChangeBadge, ChangeNote } from '@/components/common/ChangeBadge';

type SortColumn = 'name' | 'value' | 'appreciation' | 'rent' | 'equity';
type Modal = 'edit' | 'revalue' | 'delete' | null;

/**
 * The linked loans of each property, in the properties' order (`useQueries`' `combine`). A function
 * of the module, so its reference is stable: TanStack then shares the combined array structurally,
 * and it changes only when a property's loans do — one fixed-size dependency for the rows.
 */
const loanLists = (results: UseQueryResult<Loan[]>[]) => results.map((result) => result.data);

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
  const { currencyCode, fxReady } = useCurrency();
  const fmt = useFormat();
  const money = (value: number) => fmt.money(value, currencyCode, { decimals: 0 });
  const signedMoney = (value: number) =>
    fmt.money(value, currencyCode, { signed: true, decimals: 0 });
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
  const loansByProperty = useQueries({
    queries: properties.map((p) => ({
      queryKey: ['real-estate-loans', p.id],
      queryFn: () => realEstateApi.getLoans(p.id),
      staleTime: 60 * 1000,
    })),
    combine: loanLists,
  });

  // Every property's estimates: the revaluation marks of the trend card
  const { data: valuations } = useAllRealEstateValuations();

  // Today's amounts at the current quotes, each purchase price (and each purchase and
  // estimate mark of the trend) at its own day
  const convert = useMainConvert();
  const firstDay = useMemo(
    () => firstRealEstateDay(properties, valuations ?? []),
    [properties, valuations]
  );
  const dated = useDatedMainConvert(firstDay);

  const rows = useMemo(
    (): PropertyRow[] =>
      properties.map((p, i) =>
        propertyRow(p, loansByProperty[i] ?? [], excludePersonal, convert, dated.convertAt)
      ),
    [properties, excludePersonal, convert, dated.convertAt, loansByProperty]
  );

  const counted = rows.filter((r) => !r.excluded);
  const totals = propertiesSummary(rows);
  const personalValue = totals.personalValue;
  // The purchase-day rates load after the page: until then purchases are pending, not missing
  const missing = dated.ready
    ? [...new Set([...totals.missing, ...totals.missingPurchase])]
    : totals.missing;
  // Today's point of the trend card: every property (personal included, like the recorded
  // history) through the same converter as the page; a property without a rate is named.
  const trendRows = rows.filter((r) => r.value !== null);
  const trendValue = trendRows.reduce((sum, r) => sum + (r.value ?? 0), 0);
  const trendMissing = [
    ...new Set(rows.filter((r) => r.value === null).map((r) => r.valueNative.currency)),
  ];
  // Purchases and revaluations as events on the trend, in the report currency at the rates of
  // their own day (like the trade marks of the stocks list)
  const trendEvents = useMemo(
    () => realEstateEvents(properties, valuations ?? [], dated.convertAt),
    [properties, valuations, dated.convertAt]
  );
  const trendEventTip = (cluster: EventCluster<RealEstateEvent>) => {
    if (cluster.events.length > 1) {
      const first = cluster.events[0].t;
      const last = cluster.events[cluster.events.length - 1].t;
      return {
        title: t('chart.events.cluster', { count: cluster.events.length }),
        lines: [`${fmt.day(first)} – ${fmt.day(last)}`],
      };
    }
    const event = cluster.events[0];
    // The amount at the day's rate; native while that day has no rate
    const amount =
      event.value !== null
        ? money(event.value)
        : fmt.money(event.native.amount, event.native.currency, { decimals: 0 });
    const lines = [`${fmt.day(event.t)} · ${amount}`];
    if (event.change !== null) {
      lines.push(
        t('chart.events.change', { percent: fmt.percent(event.change, 1, { signed: true }) })
      );
    }
    return {
      title: t(`chart.events.${event.kind}`, { name: event.name }),
      lines,
    };
  };
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
      queryClient.invalidateQueries({ queryKey: ALL_REAL_ESTATE_VALUATIONS_KEY });
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
        case 'value':
          c = compareConverted(a.value, b.value);
          break;
        case 'appreciation':
          c = compareConverted(a.appreciation, b.appreciation);
          break;
        case 'rent':
          c = compareConverted(a.rentYear, b.rentYear);
          break;
        case 'equity':
          c = compareConverted(a.equity, b.equity);
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

  // Head of a fixed-layout column: a label that does not fit ends in "…" (the full text is
  // in the title) and the sort icon keeps its place, instead of the label widening the column.
  const sortProps = { sortColumn, sortDirection, onSort: handleSort, truncate: true };

  const isEmpty = !isLoading && properties.length === 0;
  /** First line of a numeric cell: one line, the full text in the title. */
  const main = (text: string, className?: string) => (
    <span className={cn('block truncate', className)} title={text}>
      {text}
    </span>
  );
  /** The purchase price in the main currency at its day, or native while that day has no rate. */
  const purchaseText = (r: PropertyRow) =>
    r.purchase !== null
      ? money(r.purchase)
      : fmt.money(r.purchaseNative.amount, r.purchaseNative.currency, { decimals: 0 });
  /** Appreciation as a ratio of the purchase price (0 without a known price). */
  const appreciationRatio = (r: PropertyRow) =>
    r.appreciation !== null && r.purchase !== null && r.purchase > 0
      ? r.appreciation / r.purchase
      : 0;
  /** Muted second line of a cell: one line, the full text in the title. */
  const sub = (text: string, title: string = text) => (
    <small className="mt-[3px] block truncate text-micro font-500 text-ink-4" title={title}>
      {text}
    </small>
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

      {isLoading || !fxReady ? (
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
            <div className="flex flex-col items-center gap-4">
              <AddRealEstateModal
                trigger={
                  <Button>
                    <Plus />
                    {t('addProperty')}
                  </Button>
                }
              />
              <McpTip domain="realEstate" />
            </div>
          }
        />
      ) : (
        <>
          <ConversionQualityNote quality={{ missingCurrencies: missing }} currency={currencyCode} />
          <Stats>
            <Stat
              label={t('summary.marketValue')}
              value={money(totals.value)}
              note={[
                t('summary.properties', { count: counted.length }),
                personalValue > 0
                  ? excludePersonal
                    ? t('summary.excludedNote', { amount: money(personalValue) })
                    : t('summary.includedNote', { amount: money(personalValue) })
                  : null,
              ]
                .filter(Boolean)
                .join(' · ')}
            />
            <Stat
              label={t('summary.appreciationTitle')}
              value={signedMoney(totals.appreciation)}
              tone={totals.appreciation >= 0 ? 'gain' : 'loss'}
              noteTone="neutral"
              note={
                <ChangeNote
                  ratio={totals.purchase > 0 ? totals.appreciation / totals.purchase : 0}
                  render={(percent) =>
                    t('summary.vsPurchasePct', { percent, amount: money(totals.purchase) })
                  }
                />
              }
            />
            <Stat
              label={t('summary.equity')}
              value={money(totals.equity)}
              note={
                totals.loans > 0
                  ? t('summary.equityNote', {
                      amount: money(totals.loans),
                      ltv: fmt.percent(
                        totals.equityValue > 0 ? totals.loans / totals.equityValue : 0,
                        0
                      ),
                    })
                  : t('summary.noLoans')
              }
            />
            <Stat
              label={t('summary.grossRentYear')}
              value={signedMoney(totals.rent)}
              tone={totals.rent > 0 ? 'gain' : 'neutral'}
              noteTone="neutral"
              note={
                totals.rent > 0
                  ? t('summary.rentNote', {
                      monthly: money(totals.rent / 12),
                      yield: fmt.percent(totals.rented > 0 ? totals.rent / totals.rented : 0, 1),
                    })
                  : t('summary.noRent')
              }
            />
          </Stats>

          <PortfolioTrendCard<RealEstateEvent>
            type="realEstate"
            currentValue={trendValue}
            missingCurrencies={trendMissing}
            events={trendEvents}
            renderEventTip={trendEventTip}
            legend={
              <ChartLegend
                items={[
                  { label: t('chart.legend.value'), swatch: { kind: 'line' } },
                  { label: t('chart.legend.purchase'), swatch: { kind: 'event', type: 'buy' } },
                  {
                    label: t('chart.legend.revaluation'),
                    swatch: { kind: 'event', type: 'mark' },
                  },
                ]}
              />
            }
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
            {/*
              Fixed layout, so the table never scrolls sideways: a column keeps its width and long
              text ends in "…" (the full text stays in the title). The card is 718 px wide in the
              1080 px minimum window and 1,038 px in the default 1,400 px one. The figure columns
              are sized for the widest content they show (an 8-digit amount, the longest header).
              Narrower than 1,240 px the rent column steps aside (rent and yield are in the stats
              above), so the name column keeps ≈ 37 % and the property stays readable; from
              1,240 px all four figure columns take 65 % and the name ≈ 30 %.
            */}
            <Table className="table-fixed">
              <TableHeader>
                <TableRow>
                  <SortableTableHead {...sortProps} column="name" align="left">
                    {t('table.property')}
                  </SortableTableHead>
                  <SortableTableHead
                    {...sortProps}
                    column="value"
                    className="w-[22.5%] min-[1240px]:w-[18.5%]"
                  >
                    {t('table.marketValue')}
                  </SortableTableHead>
                  <SortableTableHead
                    {...sortProps}
                    column="appreciation"
                    className="w-[18.2%] min-[1240px]:w-[15.5%]"
                  >
                    {t('table.appreciation')}
                  </SortableTableHead>
                  <SortableTableHead
                    {...sortProps}
                    column="rent"
                    className="hidden w-[16.5%] min-[1240px]:table-cell"
                  >
                    {t('table.rentYield')}
                  </SortableTableHead>
                  <SortableTableHead
                    {...sortProps}
                    column="equity"
                    className="w-[16.3%] min-[1240px]:w-[14.5%]"
                  >
                    {t('table.equity')}
                  </SortableTableHead>
                  <TableHead className="w-11">
                    <span className="sr-only">{t('table.actions')}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {visible.length === 0 ? (
                  <TableRow className="hover:bg-transparent">
                    <TableCell colSpan={6} className="py-12">
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
                        className={cn('cursor-pointer', r.excluded && 'text-ink-4')}
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
                              <div className="flex items-center gap-[6px]">
                                <b
                                  className={cn(
                                    'truncate text-table font-650',
                                    r.excluded ? 'text-ink-3' : 'text-ink'
                                  )}
                                  title={r.excluded ? `${p.name} · ${t('table.excluded')}` : p.name}
                                >
                                  {p.name}
                                </b>
                                {r.excluded && (
                                  <Badge variant="outline" className="-my-px shrink-0">
                                    {t('table.excluded')}
                                  </Badge>
                                )}
                              </div>
                              {sub([t(`types.${p.type}`), p.address].filter(Boolean).join(' · '))}
                            </div>
                          </div>
                        </TableCell>
                        <TableCell className="text-right num">
                          <ConvertedMoney
                            className={cn(
                              'block truncate font-650',
                              r.excluded ? 'text-ink-3' : 'text-ink'
                            )}
                            value={r.value}
                            amount={r.valueNative.amount}
                            currency={r.valueNative.currency}
                          />
                          {/* A purchase price of 0 is the form's "not entered" */}
                          {/* The purchase date only goes to the tooltip: the column has no room */}
                          {r.purchaseNative.amount > 0 &&
                            sub(
                              t('table.boughtFor', { amount: purchaseText(r) }),
                              p.purchaseDate !== null
                                ? t('detail.boughtOnFor', {
                                    date: fmt.day(p.purchaseDate),
                                    amount: purchaseText(r),
                                  })
                                : undefined
                            )}
                        </TableCell>
                        <TableCell className="text-right num">
                          {r.appreciation === null ? (
                            main('—', 'text-ink-4')
                          ) : r.excluded ? (
                            <>
                              {main(signedMoney(r.appreciation), 'font-650')}
                              {sub(fmt.percent(appreciationRatio(r), 1, { signed: true }))}
                            </>
                          ) : (
                            <>
                              <ChangeBadge ratio={appreciationRatio(r)} />
                              {sub(signedMoney(r.appreciation))}
                            </>
                          )}
                        </TableCell>
                        <TableCell className="hidden text-right num min-[1240px]:table-cell">
                          {r.rentMonthNative.amount > 0 ? (
                            <>
                              <ConvertedMoney
                                className="block truncate"
                                value={r.rentYear !== null ? r.rentYear / 12 : null}
                                amount={r.rentMonthNative.amount}
                                currency={r.rentMonthNative.currency}
                              />
                              {r.grossYield !== null &&
                                sub(
                                  t('table.monthlyYield', { yield: fmt.percent(r.grossYield, 1) })
                                )}
                            </>
                          ) : (
                            main(t('table.noRent'), 'text-ink-4')
                          )}
                        </TableCell>
                        <TableCell className="text-right num">
                          {r.equity === null
                            ? main('—', 'text-ink-4')
                            : main(
                                money(r.equity),
                                cn('font-650', r.excluded ? 'text-ink-3' : 'text-ink')
                              )}
                          {sub(
                            r.loans === null || r.loans > 0
                              ? r.ltv !== null
                                ? t('table.ltv', { ltv: fmt.percent(r.ltv, 0) })
                                : '—'
                              : t('table.noLoan')
                          )}
                        </TableCell>
                        <TableCell className="px-1.5 text-right">
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
                {t('table.footer.value', { amount: money(totals.value) })} ·{' '}
                {t('table.footer.equity', { amount: money(totals.equity) })}
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
                currency: recordCurrency(selected.marketPriceCurrency),
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
