import { useMemo, useState } from 'react';
import { Link, useLocation, useRoute } from 'wouter';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Ellipsis, Lock, LockOpen, Pencil, Trash2, Upload } from 'lucide-react';
import type { RealEstate } from '@shared/schema';
import type { CurrencyCode } from '@shared/currencies';
import { insuranceApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { limitsByValue, nextPaymentDay, paidSeries } from '@/utils/insurance';
import { clipSeries } from '@/utils/loan-trajectory';
import { useInsurance, useInsuranceMutations } from '@/hooks/use-insurance';
import { useDurationText } from '@/hooks/use-duration-text';
import { useShellPage } from '@/components/shell/shell-context';
import { BackLink, PageHead } from '@/components/shell/PageHead';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { Stat, Stats } from '@/components/common/Stat';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { MoonyLineChart } from '@/components/charts/MoonyLineChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Segmented } from '@/components/ui/segmented';
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
import { InsuranceFormDialog } from '@/components/insurance/InsuranceFormDialog';
import { InsuranceDocuments } from '@/components/insurance/InsuranceDocuments';
import { AddDocumentModal } from '@/components/insurance/AddDocumentModal';
import { EndPolicyDialog } from '@/components/insurance/EndPolicyDialog';
import { cn } from '@/lib/utils';

type Period = '12' | 'all';
const DAY = 86_400;

/**
 * Policy detail (design system §7 Detail, prototype insurance-detail.html):
 * cumulative premiums with the next twelve payments dashed, coverage next to
 * it, four stats (yearly cost, next payment, anniversary, duration), the
 * limits and documents tables, the contract card and the notes.
 */
export default function InsuranceDetail() {
  const [, params] = useRoute('/insurance/:id');
  const [, setLocation] = useLocation();
  const id = params?.id;
  const { t } = useTranslation('insurance');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { formatCurrency, convert, currencyCode } = useCurrency();
  const duration = useDurationText('insurance');
  const { rows, metrics, isLoading, today } = useInsurance();
  const { setStatus, remove } = useInsuranceMutations();

  const [period, setPeriod] = useState<Period>('all');
  const [editOpen, setEditOpen] = useState(false);
  const [endOpen, setEndOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [docOpen, setDocOpen] = useState(false);

  const row = rows.find((r) => r.policy.id === id);
  const policy = row?.policy;

  const { data: linkedRealEstate } = useQuery<RealEstate | null>({
    queryKey: ['insurance-real-estate', id],
    queryFn: () => insuranceApi.getRealEstate(id!),
    enabled: !!id,
  });
  const { data: documents = [] } = useQuery({
    queryKey: ['insurance-documents', id],
    queryFn: () => insuranceApi.getDocuments(id!),
    enabled: !!id,
  });

  const currency = policy?.regularPaymentCurrency || 'CZK';
  const money = (v: number) => fmt.money(v, currency, { decimals: 0 });
  const series = useMemo(() => (policy ? paidSeries(policy, today, 12) : null), [policy, today]);

  useShellPage({
    crumb: policy?.policyName,
    status: row
      ? row.nextPayment !== null
        ? {
            text: t('status.nextPayment', {
              date: fmt.day(row.nextPayment),
              amount: money(Number(policy!.regularPayment) || 0),
            }),
            tone: 'neutral',
          }
        : { text: t(row.ended ? 'status.ended' : 'status.none'), tone: 'neutral' }
      : undefined,
  });

  if (isLoading) {
    return (
      <>
        <BackLink href="/insurance">{t('detail.backToList')}</BackLink>
        <div className="mb-[26px]">
          <div className="skeleton h-2.5 w-32" />
          <div className="skeleton mt-3 h-9 w-72" />
          <div className="skeleton mt-3 h-3 w-96" />
        </div>
        <Card className="px-6 pb-4 pt-6">
          <div className="skeleton h-3 w-24" />
          <div className="skeleton mt-3 h-10 w-56" />
          <div className="skeleton mt-6 h-[200px] w-full" />
        </Card>
      </>
    );
  }
  if (!row || !policy || !series) {
    return (
      <>
        <BackLink href="/insurance">{t('detail.backToList')}</BackLink>
        <p className="mt-8 text-center text-ink-3">{t('detail.notFound')}</p>
      </>
    );
  }

  // ---- numbers ----
  const regular = Number(policy.regularPayment) || 0;
  const oneTime = Number(policy.oneTimePayment) || 0;
  const yearly = row.yearlyCzk;
  const yearlyNative = convert(yearly, 'CZK', currency as CurrencyCode);
  const coverageDisplay = convert(row.coverageCzk, 'CZK', currencyCode as CurrencyCode);
  const share = metrics.yearlyTotal > 0 ? yearly / metrics.yearlyTotal : 0;
  const startDay = series.past[0]?.t ?? policy.startDate;
  const next = row.nextPayment;
  const afterNext = next !== null ? nextPaymentDay(policy, next) : null;
  const isEnd = row.endDay !== null && (row.anniversary === null || row.endDay <= row.anniversary);
  const frequencyLabel = t(`frequencyShort.${row.frequency ?? 'monthly'}`);
  const typeLabel = t(`typeShort.${policy.type}`, { defaultValue: policy.type });
  const durationEnd = row.ended && row.endDay !== null ? Math.min(row.endDay, today) : today;

  // ---- trace window ----
  const all = [...series.past, ...series.future];
  const winStart = period === 'all' ? startDay : Math.max(startDay, today - 365 * DAY);
  const winEnd = all[all.length - 1]?.t ?? today;
  const points = clipSeries(all, winStart, winEnd);

  const kv = (label: string, value: React.ReactNode, faint = false) => (
    <div className="grid grid-cols-[1fr_auto] gap-x-4 border-b border-line-soft py-[9px] text-table text-ink-3 last:border-0">
      <span>{label}</span>
      <b className={cn('text-right font-600 num', faint ? 'text-ink-4' : 'text-ink')}>{value}</b>
    </div>
  );

  // Ranked by their CZK value: limits are in their own currencies
  const limits = limitsByValue(policy.limits);

  return (
    <>
      <BackLink href="/insurance">{t('detail.backToList')}</BackLink>
      <PageHead
        eyebrow={[
          typeLabel,
          policy.provider,
          policy.policyNumber ? t('detail.policyNumber', { number: policy.policyNumber }) : null,
        ].filter(Boolean)}
        title={policy.policyName}
        description={
          <>
            {t(row.frequency === 'one_time' ? 'detail.descriptionOneTime' : 'detail.description', {
              start: fmt.day(policy.startDate),
              end:
                policy.endDate !== null
                  ? t('detail.until', { date: fmt.day(policy.endDate) })
                  : t('detail.noEnd'),
              premium: money(row.frequency === 'one_time' ? oneTime : regular),
              frequency: frequencyLabel,
            })}{' '}
            ·{' '}
            <Badge variant={row.ended ? 'default' : 'gain'} className="align-[1px]">
              {t(row.ended ? 'detail.statusEnded' : 'detail.statusActive')}
            </Badge>
          </>
        }
        actions={
          <>
            <Button variant="outline" onClick={() => setEditOpen(true)}>
              <Pencil />
              {t('actions.edit')}
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="icon" aria-label={tc('labels.moreActions')}>
                  <Ellipsis />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                {row.ended ? (
                  <DropdownMenuItem onSelect={() => setStatus.mutate({ policy, status: 'active' })}>
                    <LockOpen />
                    {t('actions.reactivate')}
                  </DropdownMenuItem>
                ) : (
                  <DropdownMenuItem onSelect={() => setEndOpen(true)}>
                    <Lock />
                    {t('actions.end')}
                  </DropdownMenuItem>
                )}
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="danger" onSelect={() => setDeleteOpen(true)}>
                  <Trash2 />
                  {t('actions.delete')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <Button onClick={() => setDocOpen(true)}>
              <Upload />
              {t('actions.addDocument')}
            </Button>
          </>
        }
      />

      <HeroCard
        aside={
          row.frequency !== 'one_time' ? (
            <Segmented
              value={period}
              onValueChange={setPeriod}
              options={[
                { value: '12', label: t('detail.period.year') },
                { value: 'all', label: t('detail.period.all') },
              ]}
            />
          ) : undefined
        }
        chart={
          points.length > 1 ? (
            <>
              <MoonyLineChart
                className="-mx-1 mt-[18px]"
                points={points}
                height={200}
                zeroBaseline
                projectedFrom={today}
                formatValue={money}
                renderTip={(p) => ({
                  title: money(p.value),
                  lines: [
                    `${fmt.day(p.t)} · ${
                      p.t > today ? t('detail.chart.tipPlanned') : t('detail.chart.tipPaid')
                    }`,
                  ],
                })}
              />
              <ChartLegend
                items={[
                  { label: t('detail.chart.paid'), swatch: { kind: 'line' } },
                  ...(series.future.length > 0
                    ? [
                        {
                          label: t('detail.chart.next12'),
                          swatch: { kind: 'dash' as const, color: 'var(--chart-line)' },
                        },
                      ]
                    : []),
                ]}
                note={
                  row.frequency === 'one_time'
                    ? t('detail.chart.noteOneTime', { date: fmt.day(policy.startDate) })
                    : t('detail.chart.note', {
                        payments: t('payments', { count: series.paymentsMade }),
                        yearly: money(yearlyNative),
                      })
                }
              />
            </>
          ) : undefined
        }
      >
        <HeroValue
          label={t('detail.hero.paid')}
          value={money(series.paidToDay)}
          deltaNote={
            row.frequency === 'one_time'
              ? t('detail.hero.paidNoteOneTime', { date: fmt.day(policy.startDate) })
              : t('detail.hero.paidNote', {
                  payments: t('payments', { count: series.paymentsMade }),
                  month: fmt.month(new Date(policy.startDate * 1000), {
                    month: 'numeric',
                    year: 'numeric',
                  }),
                  yearly: money(yearlyNative),
                })
          }
        />
        <HeroValue
          compact
          label={t('detail.hero.coverage')}
          value={
            row.coverageCzk > 0 ? fmt.money(coverageDisplay, currencyCode, { decimals: 0 }) : '—'
          }
          deltaNote={
            row.coverageCzk > 0
              ? yearly > 0
                ? t('detail.hero.coverageRatio', {
                    ratio: fmt.number(row.coverageCzk / yearly, { maximumFractionDigits: 0 }),
                  })
                : t('detail.hero.coverageOneTime', {
                    count: limits.length,
                  })
              : t('detail.hero.noCoverage')
          }
        />
      </HeroCard>

      <Stats className="mt-[17px]">
        <Stat
          label={t('detail.stats.yearly')}
          value={yearly > 0 ? money(yearlyNative) : money(oneTime)}
          note={
            yearly > 0
              ? t('detail.stats.yearlyNote', {
                  premium: money(regular),
                  frequency: frequencyLabel,
                  share: fmt.percent(share, 0),
                })
              : t('detail.stats.yearlyOneTime')
          }
        />
        <Stat
          label={t('detail.stats.nextPayment')}
          value={next !== null ? fmt.day(next) : '—'}
          note={
            next !== null
              ? t('detail.stats.nextPaymentNote', {
                  in: duration.ahead(today, next),
                  amount: money(regular),
                  next: afterNext !== null ? fmt.day(afterNext) : '—',
                })
              : t('detail.stats.noNextPayment')
          }
        />
        <Stat
          label={t(isEnd ? 'detail.stats.end' : 'detail.stats.anniversary')}
          value={
            row.ended
              ? row.endDay !== null
                ? fmt.day(row.endDay)
                : '—'
              : isEnd
                ? fmt.day(row.endDay!)
                : row.anniversary !== null
                  ? fmt.day(row.anniversary)
                  : '—'
          }
          note={
            row.ended
              ? t('detail.stats.endedNote', {
                  date: row.endDay !== null ? fmt.day(row.endDay) : '',
                })
              : isEnd
                ? t('detail.stats.endNote', { in: duration.ahead(today, row.endDay!) })
                : row.anniversary !== null
                  ? t('detail.stats.anniversaryNote', {
                      in: duration.ahead(today, row.anniversary),
                    })
                  : ''
          }
        />
        <Stat
          label={t('detail.stats.duration')}
          value={duration.between(policy.startDate, durationEnd)}
          note={t(
            policy.endDate !== null ? 'detail.stats.durationEnd' : 'detail.stats.durationNote',
            {
              date: policy.endDate !== null ? fmt.day(policy.endDate) : '',
              docs: t('documentsCount', { count: documents.length }),
            }
          )}
        />
      </Stats>

      <div className="mt-[17px] grid grid-cols-[1.4fr_0.6fr] items-start gap-[18px]">
        <div className="grid gap-[14px]">
          <Card variant="table">
            <CardHeader>
              <div>
                <CardTitle>{t('detail.limits.title')}</CardTitle>
                <CardDescription>{t('detail.limits.subtitle')}</CardDescription>
              </div>
              <Button variant="outline" size="sm" onClick={() => setEditOpen(true)}>
                <Pencil />
                {t('detail.limits.edit')}
              </Button>
            </CardHeader>
            {limits.length === 0 ? (
              <p className="border-t border-line px-[14px] py-8 text-center text-table text-ink-3">
                {t('detail.limits.empty')}
              </p>
            ) : (
              <>
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t('detail.limits.risk')}</TableHead>
                      <TableHead className="text-right">{t('detail.limits.limit')}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {limits.map((limit, index) => (
                      <TableRow key={`${limit.title}-${index}`} className="h-12">
                        <TableCell className="font-600 text-ink">{limit.title}</TableCell>
                        <TableCell className="text-right num font-650 text-ink">
                          {fmt.money(Number(limit.amount) || 0, limit.currency || currency, {
                            decimals: 0,
                          })}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
                <div className="flex items-center justify-between gap-6 border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
                  <span>
                    {t('detail.limits.footer', {
                      risks: t('risks', { count: limits.length }),
                      amount: formatCurrency(row.coverageCzk),
                    })}
                  </span>
                </div>
              </>
            )}
          </Card>
          <InsuranceDocuments insuranceId={policy.id} onAdd={() => setDocOpen(true)} />
        </div>

        <aside className="grid gap-[14px]">
          <Card>
            <CardHeader>
              <CardTitle className="text-[16px]">{t('detail.contract.title')}</CardTitle>
              <Badge variant={row.ended ? 'default' : 'gain'}>
                {t(row.ended ? 'detail.statusEnded' : 'detail.statusActive')}
              </Badge>
            </CardHeader>
            <CardContent className="pt-0">
              {kv(t('detail.contract.provider'), policy.provider)}
              {kv(t('detail.contract.number'), policy.policyNumber || '—', !policy.policyNumber)}
              {kv(t('detail.contract.type'), typeLabel)}
              {kv(t('detail.contract.start'), fmt.day(policy.startDate))}
              {kv(
                t('detail.contract.end'),
                policy.endDate !== null ? fmt.day(policy.endDate) : t('detail.contract.endNone'),
                policy.endDate === null
              )}
              {kv(t('detail.contract.frequency'), frequencyLabel)}
              {kv(t('detail.contract.premium'), money(regular))}
              {kv(
                t('detail.contract.oneTime'),
                oneTime > 0
                  ? fmt.money(oneTime, policy.oneTimePaymentCurrency || currency, { decimals: 0 })
                  : t('detail.contract.oneTimeNone'),
                oneTime <= 0
              )}
              {linkedRealEstate &&
                kv(
                  t('detail.contract.property'),
                  <Link
                    href={`/real-estate/${linkedRealEstate.id}`}
                    className="underline underline-offset-[3px] hover:text-ink-2"
                  >
                    {linkedRealEstate.name}
                  </Link>
                )}
            </CardContent>
          </Card>
          <Card variant="flat">
            <CardHeader className="pb-1">
              <CardTitle className="text-[16px]">{t('detail.notes.title')}</CardTitle>
              <Button variant="ghost" size="sm" onClick={() => setEditOpen(true)}>
                {t('detail.notes.edit')}
              </Button>
            </CardHeader>
            <CardContent className="pt-0">
              {policy.notes ? (
                <>
                  <p className="m-0 whitespace-pre-wrap text-caption text-ink-2">{policy.notes}</p>
                  <p className="mb-0 mt-2 text-micro font-500 text-ink-4">
                    {t('detail.notes.updated', { date: fmt.day(policy.updatedAt) })}
                  </p>
                </>
              ) : (
                <p className="m-0 text-caption text-ink-4">{t('detail.notes.empty')}</p>
              )}
            </CardContent>
          </Card>
        </aside>
      </div>

      <InsuranceFormDialog
        key={policy.updatedAt}
        policy={policy}
        open={editOpen}
        onOpenChange={setEditOpen}
      />
      <AddDocumentModal open={docOpen} onOpenChange={setDocOpen} insuranceId={policy.id} />
      <EndPolicyDialog
        open={endOpen}
        onOpenChange={setEndOpen}
        policy={policy}
        today={today}
        isPending={setStatus.isPending}
        onConfirm={(endDay) =>
          setStatus.mutate(
            { policy, status: 'inactive', endDate: endDay },
            { onSuccess: () => setEndOpen(false) }
          )
        }
      />
      <ConfirmDeleteDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.description', { name: policy.policyName })}
        onConfirm={() =>
          remove.mutate(policy.id, {
            onSuccess: () => {
              setDeleteOpen(false);
              setLocation('/insurance');
            },
          })
        }
        isPending={remove.isPending}
        confirmLabel={t('actions.delete')}
      />
    </>
  );
}
