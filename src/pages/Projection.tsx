import { useMemo, useState } from 'react';
import { Link } from 'wouter';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Landmark, Settings2, TrendingUp } from 'lucide-react';
import type { PortfolioProjection, ProjectionSettings } from '@shared/schema';
import { projectionApi } from '@/lib/tauri-api';
import { useAuth } from '@/hooks/use-auth';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useShellPage } from '@/components/shell/shell-context';
import { horizonAxisLabels, payoffIndex, roundMilestones } from '@/utils/projection-milestones';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Segmented } from '@/components/ui/segmented';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { SeriesDot, Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { EmptyState } from '@/components/common/EmptyState';
import {
  MoonyBandChart,
  type BandMilestone,
  type BandPoint,
} from '@/components/charts/MoonyBandChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { ProjectionParamsDialog } from '@/components/projection/ProjectionParamsDialog';
import { resolveClassParams } from '@/components/projection/projection-classes';
import { cn } from '@/lib/utils';

const HORIZONS = ['5', '10', '20', '30'] as const;
type Horizon = (typeof HORIZONS)[number];
/** Scenario band: every active rate ± this many percentage points. */
const SHIFT = 2;

interface Band {
  lo: PortfolioProjection;
  mid: PortfolioProjection;
  hi: PortfolioProjection;
}

/**
 * Net-worth projection (design system §7 Outlook, prototype `projection.html`):
 * horizon segment, hero with the expected line, the ± 2 p.p. scenario band,
 * the dashed "today + contributions" reference and milestones, four stats,
 * the asset-class table, three scenarios and the parameters modal.
 */
export default function Projection() {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { formatCurrency, formatCurrencyShort, formatCurrencySigned } = useCurrency();
  const { user } = useAuth();
  const queryClient = useQueryClient();

  const [horizon, setHorizon] = useState<Horizon>('20');
  const [paramsOpen, setParamsOpen] = useState(false);
  const years = Number(horizon);
  // Monthly points keep the short horizons smooth; yearly points suffice beyond
  const viewType = years <= 10 ? 'monthly' : 'yearly';
  const monthsPerPoint = viewType === 'monthly' ? 1 : 12;
  const excludePersonalRE = user?.excludePersonalRealEstate ?? false;

  const { data: settings = [] } = useQuery<ProjectionSettings[]>({
    queryKey: ['projection-settings'],
    queryFn: () => projectionApi.getSettings(),
  });

  // Three runs of the same model: expected, and every active rate ± 2 p.p.
  const { data: band, isLoading } = useQuery<Band>({
    queryKey: ['projection', years, viewType, excludePersonalRE, 'band'],
    queryFn: async () => {
      const run = (rateShift: number) =>
        projectionApi.calculate({
          horizonYears: years,
          viewType,
          excludePersonalRealEstate: excludePersonalRE,
          rateShift,
        });
      const [lo, mid, hi] = await Promise.all([run(-SHIFT), run(0), run(SHIFT)]);
      return { lo, mid, hi };
    },
    refetchOnMount: 'always',
  });

  const saveMutation = useMutation({
    mutationFn: (next: ProjectionSettings[]) => projectionApi.saveSettings(next),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projection-settings'] });
      queryClient.invalidateQueries({ queryKey: ['projection'] });
      toast.success(t('projection.dialog.saved'));
      setParamsOpen(false);
    },
    onError: (error: Error) => toast.error(error.message),
  });

  useShellPage({
    status: { text: t('projection.status', { date: fmt.date(new Date()) }), tone: 'neutral' },
  });

  const params = useMemo(
    () =>
      resolveClassParams(
        settings,
        band?.mid.calculatedDefaults ?? { savingsRate: 0, bondsRate: 0 }
      ),
    [settings, band]
  );

  const timeline = useMemo(() => band?.mid.timeline ?? [], [band]);
  const monthlyContributions = params.reduce((s, p) => s + (p.enabled ? p.contribution : 0), 0);

  const points = useMemo<BandPoint[]>(() => {
    if (!band) return [];
    const first = band.mid.timeline[0];
    return band.mid.timeline.map((p, i) => ({
      x: i,
      value: p.netWorth,
      lo: band.lo.timeline[i]?.netWorth ?? p.netWorth,
      hi: band.hi.timeline[i]?.netWorth ?? p.netWorth,
      // Today's assets plus the contributions paid in so far, minus the loans at that point
      base: first.totalAssets + monthlyContributions * i * monthsPerPoint - p.totalLiabilities,
    }));
  }, [band, monthlyContributions, monthsPerPoint]);

  const yearOf = (index: number) =>
    timeline[index] ? fmt.day(timeline[index].date, { year: 'numeric' }) : '';

  const payoff = useMemo(
    () =>
      payoffIndex(timeline.map((p) => ({ value: p.netWorth, liabilities: p.totalLiabilities }))),
    [timeline]
  );
  const milestones = useMemo<BandMilestone[]>(() => {
    const list: BandMilestone[] = [];
    if (payoff !== null) {
      list.push({
        x: payoff,
        title: t('projection.milestones.payoff'),
        lines: [t('projection.milestones.payoffLine', { year: yearOf(payoff) })],
      });
    }
    for (const m of roundMilestones(
      timeline.map((p) => p.netWorth),
      payoff !== null ? [payoff] : []
    )) {
      list.push({
        x: m.index,
        title: formatCurrencyShort(m.value),
        lines: [t('projection.milestones.roundLine', { year: yearOf(m.index) })],
      });
    }
    return list;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [timeline, payoff, fmt.locale]);

  const head = (
    <PageHead
      eyebrow={t('projection.eyebrow')}
      title={t('projection.title')}
      description={t('projection.subtitle')}
      actions={
        <>
          <Segmented
            size="lg"
            value={horizon}
            onValueChange={setHorizon}
            options={HORIZONS.map((h) => ({
              value: h,
              label: t('projection.horizon', { count: Number(h) }),
            }))}
          />
          <Button variant="outline" onClick={() => setParamsOpen(true)}>
            <Settings2 />
            {t('projection.params')}
          </Button>
        </>
      }
    />
  );

  const dialog = (
    <ProjectionParamsDialog
      open={paramsOpen}
      onOpenChange={setParamsOpen}
      params={params}
      onSave={(next) => saveMutation.mutate(next)}
      isPending={saveMutation.isPending}
    />
  );

  if (isLoading || !band) {
    return (
      <>
        {head}
        <Stats columns={4}>
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
        </Stats>
      </>
    );
  }

  const first = timeline[0];
  const last = timeline[timeline.length - 1];
  const today = first?.netWorth ?? 0;
  const projected = last?.netWorth ?? 0;
  const contributions = band.mid.totalContributions;

  // Nothing to project: no assets, no liabilities and no configured contributions
  if (
    (first?.totalAssets ?? 0) === 0 &&
    (first?.totalLiabilities ?? 0) === 0 &&
    projected === 0 &&
    contributions === 0
  ) {
    return (
      <>
        {head}
        <EmptyState
          icon={<TrendingUp />}
          title={t('projection.empty.title')}
          description={t('projection.empty.description')}
          action={
            <div className="flex flex-wrap items-center justify-center gap-2">
              <Button asChild>
                <Link href="/bank-accounts">
                  <Landmark />
                  {t('projection.empty.bankAccounts')}
                </Link>
              </Button>
              <Button asChild variant="outline">
                <Link href="/stocks">{t('projection.empty.stocks')}</Link>
              </Button>
            </div>
          }
        />
        {dialog}
      </>
    );
  }

  const money = (v: number) => formatCurrency(v);
  const short = (v: number) => formatCurrencyShort(v);
  const delta = projected - today;
  const lastLo = band.lo.timeline[band.lo.timeline.length - 1]?.netWorth ?? projected;
  const lastHi = band.hi.timeline[band.hi.timeline.length - 1]?.netWorth ?? projected;
  const assetGrowth = (last?.totalAssets ?? 0) - (first?.totalAssets ?? 0);
  const returns = assetGrowth - contributions;
  const share = (part: number) => (delta > 0 ? fmt.percent(part / delta, 0) : null);

  // Weighted expected return over today's active holdings
  const activeValue = params.reduce(
    (s, p) => s + (p.enabled && first ? Math.max(0, first[p.cls.field]) : 0),
    0
  );
  const weightedRate =
    activeValue > 0
      ? params.reduce(
          (s, p) => s + (p.enabled && first ? Math.max(0, first[p.cls.field]) * p.rate : 0),
          0
        ) / activeValue
      : 0;

  const startYear = new Date().getUTCFullYear();
  const activeCount = params.filter((p) => p.enabled).length;
  const scenarios = [
    { key: 'pessimistic', value: lastLo, expected: false },
    { key: 'expected', value: projected, expected: true },
    { key: 'optimistic', value: lastHi, expected: false },
  ] as const;
  const scenarioMax = Math.max(lastHi, projected, lastLo, 1);

  return (
    <>
      {head}

      <HeroCard
        aside={
          <HeroValue
            compact
            label={t('projection.hero.today')}
            value={money(today)}
            deltaNote={t('projection.hero.weightedReturn', {
              rate: fmt.percent(weightedRate / 100, 1),
            })}
          />
        }
        chart={
          <>
            <MoonyBandChart
              className="-mx-1 mt-[18px]"
              points={points}
              height={230}
              formatValue={money}
              axisLabels={horizonAxisLabels(startYear, years, tc('charts.today'))}
              milestones={milestones}
              renderTip={(p) => ({
                title: money(p.value),
                lines: [
                  t('projection.tip.scenarios', {
                    year: yearOf(p.x),
                    lo: short(p.lo),
                    hi: short(p.hi),
                  }),
                  t('projection.tip.assetsLiabilities', {
                    assets: short(timeline[p.x]?.totalAssets ?? 0),
                    liabilities: short(timeline[p.x]?.totalLiabilities ?? 0),
                  }),
                ],
              })}
            />
            <ChartLegend
              items={[
                { label: t('projection.legend.expected'), swatch: { kind: 'line' } },
                {
                  label: t('projection.legend.band'),
                  swatch: { kind: 'block', color: 'var(--s4)' },
                },
                { label: t('projection.legend.base'), swatch: { kind: 'dash' } },
                { label: t('projection.legend.milestone'), swatch: { kind: 'ring' } },
              ]}
            />
          </>
        }
      >
        <HeroValue
          label={t('projection.hero.label', { count: years })}
          value={money(projected)}
          tone={delta >= 0 ? 'gain' : 'loss'}
          delta={`${delta >= 0 ? '↗' : '↘'} ${formatCurrencySigned(delta)}`}
          deltaNote={t('projection.hero.deltaNote', { lo: short(lastLo), hi: short(lastHi) })}
        />
      </HeroCard>

      <Stats columns={4} className="mt-[17px]">
        <Stat
          label={t('projection.stats.contributions', { count: years })}
          value={money(contributions)}
          note={
            share(contributions)
              ? t('projection.stats.contributionsNote', {
                  monthly: money(monthlyContributions),
                  share: share(contributions),
                })
              : t('projection.stats.contributionsNoteNoShare', {
                  monthly: money(monthlyContributions),
                })
          }
        />
        <Stat
          label={t('projection.stats.growth')}
          value={formatCurrencySigned(returns)}
          tone={returns > 0 ? 'gain' : returns < 0 ? 'loss' : 'neutral'}
          noteTone="neutral"
          note={
            share(returns)
              ? t('projection.stats.growthNote', { share: share(returns) })
              : t('projection.stats.growthNoteNoShare')
          }
        />
        <Stat
          label={t('projection.stats.assets', { count: years })}
          value={money(last?.totalAssets ?? 0)}
          note={t('projection.stats.assetsNote', { amount: money(first?.totalAssets ?? 0) })}
        />
        <Stat
          label={t('projection.stats.liabilities', { count: years })}
          value={money(last?.totalLiabilities ?? 0)}
          note={
            (first?.totalLiabilities ?? 0) === 0
              ? t('projection.stats.noLiabilities')
              : payoff !== null
                ? t('projection.stats.paidOff', { year: yearOf(payoff) })
                : t('projection.stats.remaining')
          }
        />
      </Stats>

      <div className="grid grid-cols-[1.5fr_0.5fr] items-start gap-[18px]">
        <Card variant="table">
          <CardHeader>
            <div>
              <CardTitle>{t('projection.table.title')}</CardTitle>
              <CardDescription>{t('projection.table.sub')}</CardDescription>
            </div>
          </CardHeader>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-[28%]">{t('projection.table.class')}</TableHead>
                <TableHead className="text-right">{t('projection.table.today')}</TableHead>
                <TableHead className="text-right">{t('projection.table.growth')}</TableHead>
                <TableHead className="text-right">{t('projection.table.contribution')}</TableHead>
                <TableHead className="text-right">
                  {t('projection.table.at', { count: years })}
                </TableHead>
                <TableHead className="w-[10%] text-right">{t('projection.table.share')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {params.map((p) => {
                const now = first?.[p.cls.field] ?? 0;
                const then = last?.[p.cls.field] ?? 0;
                const totalThen = last?.totalAssets ?? 0;
                return (
                  <TableRow key={p.cls.key} className={cn('h-[50px]', !p.enabled && 'text-ink-4')}>
                    <TableCell>
                      <span className="inline-flex items-center gap-2">
                        <SeriesDot className="size-2" color={p.cls.color} />
                        <b
                          className={cn(
                            'text-caption font-650',
                            p.enabled ? 'text-ink' : 'text-ink-4'
                          )}
                        >
                          {t(`projection.categories.${p.cls.labelKey}`)}
                        </b>
                        {!p.enabled && (
                          <Badge variant="outline">{t('projection.table.excluded')}</Badge>
                        )}
                      </span>
                    </TableCell>
                    <TableCell className="text-right num">{money(now)}</TableCell>
                    <TableCell className="text-right num">
                      {p.enabled ? fmt.percent(p.rate / 100, 1) : '—'}
                    </TableCell>
                    <TableCell className="text-right num">
                      {p.enabled && p.contribution > 0 ? (
                        money(p.contribution)
                      ) : (
                        <span className="text-ink-5">—</span>
                      )}
                    </TableCell>
                    <TableCell className={cn('text-right font-650 num', p.enabled && 'text-ink')}>
                      {money(then)}
                    </TableCell>
                    <TableCell className="text-right num">
                      {fmt.percent(totalThen > 0 ? then / totalThen : 0, 0)}
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
          <div className="flex items-center justify-between gap-4 border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
            <span className="num">
              {t('projection.table.classes', { count: params.length })} ·{' '}
              {t('projection.table.active', { count: activeCount })} ·{' '}
              {t('projection.table.assetsAt', {
                count: years,
                amount: money(last?.totalAssets ?? 0),
              })}
            </span>
            <span>{t('projection.table.loansNote')}</span>
          </div>
        </Card>

        <Card variant="flat">
          <CardHeader className="pb-1.5">
            <div>
              <CardTitle className="text-[16px]">{t('projection.scenarios.title')}</CardTitle>
              <CardDescription>{t('projection.scenarios.sub', { count: years })}</CardDescription>
            </div>
          </CardHeader>
          <div className="grid gap-[14px] px-[21px] pb-5 pt-[10px]">
            {scenarios.map((s) => (
              <div
                key={s.key}
                className="grid grid-cols-[110px_1fr_auto] items-center gap-3 text-table text-ink-2"
              >
                <span>
                  {t(`projection.scenarios.${s.key}`)}
                  <small className="block text-micro font-500 text-ink-4">
                    {t(`projection.scenarios.${s.key}Note`)}
                  </small>
                </span>
                <div className="h-2 overflow-hidden rounded-[4px] bg-well-3">
                  <i
                    className={cn('block h-full rounded-[4px]', s.expected ? 'bg-s1' : 'bg-s3')}
                    style={{ width: `${Math.max(0, (s.value / scenarioMax) * 100)}%` }}
                  />
                </div>
                <b className="font-650 text-ink num">{short(s.value)}</b>
              </div>
            ))}
          </div>
        </Card>
      </div>

      {dialog}
    </>
  );
}
