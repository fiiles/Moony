import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useLocation } from 'wouter';
import {
  Bitcoin,
  Eye,
  Landmark,
  TrendingDown,
  TrendingUp,
  Wallet,
  type LucideIcon,
} from 'lucide-react';
import { useAuth } from '@/hooks/use-auth';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { useDatedMainConvert } from '@/hooks/use-fx';
import { useGreeting } from '@/hooks/use-greeting';
import { useMilestones } from '@/hooks/use-milestones';
import { ASSET_CLASSES, useNetWorthOverview } from '@/hooks/use-net-worth-overview';
import { useNetWorthJumpTip } from '@/hooks/use-net-worth-jump-tip';
import { useRecentMoves, type MoveKind, type RecentMove } from '@/hooks/use-recent-moves';
import { useFormat } from '@/lib/use-format';
import { utcDayFloor } from '@/utils/chart-axis';
import { splitMilestones } from '@/utils/milestones';
import { CHART_PERIODS, type ChartPeriod } from '@/utils/period';
import { Button } from '@/components/ui/button';
import { ConvertedMoney } from '@/components/common/ConvertedMoney';
import { EmptyState } from '@/components/common/EmptyState';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';
import { AgendaRow } from '@/mobile/components/AgendaRow';
import { AllocationRing } from '@/mobile/components/AllocationRing';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { Hero } from '@/mobile/components/Hero';
import { SectionHead } from '@/mobile/components/SectionHead';
import { StatRow, type StatItem } from '@/mobile/components/StatRow';
import { UpkeepCard } from '@/mobile/components/UpkeepCard';
import { initials } from '@/mobile/lib/initials';
import { NavIconButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/** Agenda rows shown on Přehled before "Zobrazit vše (N)". */
const AGENDA_ROWS = 3;
/** Rows of "Poslední pohyby". */
const MOVES = 5;
/** From a million on, a stat shows the compact amount ("6,78 mil. Kč") so a third holds it. */
const COMPACT_FROM = 1_000_000;

/**
 * Přehled tab root (spec §8.3, prototype frame 02): the greeting, Údržba (only while something is
 * due), the hero with the scrubbable net-worth line, three stats, "Co vás čeká", Rozložení majetku
 * and Poslední pohyby. Numbers come from the hooks the desktop dashboard uses.
 */
export default function Overview() {
  const { t } = useTranslation('mobile');
  const { t: td } = useTranslation('dashboard');
  const { t: tc } = useTranslation('common');
  const { user } = useAuth();
  const fmt = useFormat();
  const [period, setPeriod] = useState<ChartPeriod>('30D');

  const greeting = useGreeting();
  const asOf = td('asOf', {
    date: fmt.date(new Date(), { day: 'numeric', month: 'numeric', year: 'numeric' }),
  });
  const overview = useNetWorthOverview(period);
  const { reportCurrency, display, unknown, hasHistory, shortHistoryBase } = overview;
  const jumpTip = useNetWorthJumpTip(reportCurrency);

  const money = (value: number) => fmt.money(value, reportCurrency, { decimals: 0 });
  const signedMoney = (value: number) =>
    fmt.money(value, reportCurrency, { signed: true, decimals: 0 });
  const statMoney = (value: number, isUnknown: boolean) => {
    if (isUnknown) return '—';
    return Math.abs(value) >= COMPACT_FROM
      ? fmt.money(value, reportCurrency, { compact: true, decimals: 2 })
      : money(value);
  };

  // "za 30 dní", or "od 3. 10." when the history is shorter than the period
  const changeLabel =
    shortHistoryBase === null
      ? t(`overview.change.${period}`)
      : td('changeSince', {
          date: fmt.day(shortHistoryBase, {
            month: 'short',
            day: 'numeric',
            year:
              new Date(shortHistoryBase * 1000).getUTCFullYear() !== new Date().getUTCFullYear()
                ? 'numeric'
                : undefined,
          }),
        });

  const stats = useMemo<StatItem[]>(
    () => [
      {
        label: t('overview.stats.assets'),
        value: statMoney(display.assets, unknown.assets),
        note: unknown.assets
          ? undefined
          : hasHistory
            ? signedMoney(overview.assetsDelta)
            : t('overview.stats.noHistory'),
      },
      {
        label: td('stats.totalLiabilities'),
        value: statMoney(display.liabilities, unknown.liabilities),
        note: unknown.liabilities
          ? undefined
          : !hasHistory
            ? t('overview.stats.noHistory')
            : Math.abs(overview.liabilitiesDelta) < 1
              ? t('overview.stats.unchanged')
              : signedMoney(overview.liabilitiesDelta),
      },
      {
        label: t('overview.stats.freeCash'),
        value: statMoney(display.savings, false),
        note: t('overview.stats.freeCashNote'),
      },
    ],
    // The formatters close over the same inputs; the metrics response drives every value
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [
      overview.portfolioMetrics,
      hasHistory,
      overview.assetsDelta,
      overview.liabilitiesDelta,
      t,
      td,
      fmt,
    ]
  );

  const avatar = `${user?.name?.charAt(0) ?? ''}${user?.surname?.charAt(0) ?? ''}`.toUpperCase();

  return (
    <Screen
      title={greeting}
      large
      subtitle={asOf}
      leading={
        // Task 1.9 wires the hide-amounts eye; the slot holds its place until then.
        <NavIconButton aria-label={t('overview.hideAmounts')}>
          <Eye strokeWidth={1.75} aria-hidden />
        </NavIconButton>
      }
      trailing={
        <Link
          href={routes.settings}
          aria-label={tc('nav.settings')}
          className="inline-flex size-11 flex-none items-center justify-center rounded-[12px] [-webkit-touch-callout:none] active:bg-well-2"
        >
          <span
            aria-hidden
            className="inline-flex size-8 items-center justify-center rounded-full bg-well-2 text-m-caption font-bold text-ink-2 shadow-[inset_0_0_0_1px_var(--line)]"
          >
            {avatar}
          </span>
        </Link>
      }
    >
      {/* First on the page and only while something is due (it also asks for a first backup) */}
      {!overview.isCheckingPortfolio && <UpkeepCard />}
      {overview.isCheckingPortfolio ? (
        <OverviewSkeleton />
      ) : overview.isEmptyPortfolio ? (
        <EmptyOverview />
      ) : (
        <>
          {/* Amounts the backend could not convert into the report currency (left out) */}
          <ConversionQualityNote
            quality={overview.fxQuality}
            currency={reportCurrency}
            className="mx-0.5 mb-3.5"
          />
          <Hero
            label={td('hero.label')}
            value={unknown.netWorth ? null : display.netWorth}
            formatValue={money}
            change={
              hasHistory && !unknown.netWorth
                ? {
                    ratio: overview.netWorthPct,
                    text: `${signedMoney(overview.netWorthDelta)} ${changeLabel}`,
                  }
                : undefined
            }
            note={unknown.netWorth ? undefined : td('stats.noHistory')}
            period={period}
            periods={CHART_PERIODS}
            onPeriodChange={setPeriod}
            points={overview.points}
            zeroBaseline={period === 'All'}
            events={overview.jumps}
            renderEventTip={jumpTip}
            chartLabel={td('charts.netWorthTrend')}
          />
          <StatRow stats={stats} />
          <AgendaSection />
          <AllocationSection
            values={ASSET_CLASSES.map((cls) => ({
              key: cls,
              label: td(`cards.${cls}`),
              value: display[cls],
            }))}
            formatValue={money}
          />
          <RecentMovesSection />
        </>
      )}
    </Screen>
  );
}

/** "Co vás čeká" (spec §8.3 item 4): three agenda rows and "Zobrazit vše (N)"; hidden when empty. */
function AgendaSection() {
  const { t } = useTranslation('milestones');
  const { data: milestones = [] } = useMilestones();
  const { agenda } = splitMilestones(milestones, utcDayFloor(Date.now() / 1000));
  if (agenda.length === 0) return null;
  return (
    <section>
      <SectionHead
        title={t('card.title')}
        action={
          agenda.length > AGENDA_ROWS
            ? { label: t('card.showAll', { count: agenda.length }), href: routes.agenda }
            : undefined
        }
      />
      <EntityRows>
        {agenda.slice(0, AGENDA_ROWS).map((m) => (
          <AgendaRow key={m.key} m={m} />
        ))}
      </EntityRows>
    </section>
  );
}

/** Rozložení majetku (spec §8.3 item 5): the 110 pt ring and its legend; hidden without assets. */
function AllocationSection({
  values,
  formatValue,
}: {
  values: { key: string; label: string; value: number }[];
  formatValue: (value: number) => string;
}) {
  const { t } = useTranslation('mobile');
  const { t: td } = useTranslation('dashboard');
  const fmt = useFormat();
  if (!values.some((v) => v.value > 0)) return null;
  return (
    <section>
      <SectionHead
        title={td('allocation.title')}
        action={{
          label: (
            <>
              {t('tabs.wealth')} <span aria-hidden>→</span>
            </>
          ),
          href: routes.wealth,
        }}
      />
      <div className="mb-3.5 rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 py-3.5 shadow-e1">
        <AllocationRing
          segments={values}
          centerLabel={td('allocation.center')}
          otherLabel={td('cards.other')}
          formatPercent={(ratio) => fmt.percent(ratio, 0)}
          formatValue={formatValue}
        />
      </div>
    </section>
  );
}

const MOVE_ICONS: Partial<Record<MoveKind, LucideIcon>> = {
  buy: TrendingUp,
  sell: TrendingDown,
  cryptoBuy: Bitcoin,
  cryptoSell: Bitcoin,
};

/**
 * Poslední pohyby (spec §8.3 item 6): five entity rows — initials of the counterparty (an icon
 * for a trade), "Dnes · účet · kategorie", the amount in the main currency at its day's rate. Only
 * a bank credit is green with a plus; a debit carries a minus; trades are neutral amounts whose
 * direction is in the sub. Hidden when there is nothing yet.
 */
function RecentMovesSection() {
  const { t } = useTranslation('mobile');
  const { t: td } = useTranslation('dashboard');
  const { moves, isLoading } = useRecentMoves(MOVES);
  if (isLoading || moves.length === 0) return null;
  return (
    <section>
      <SectionHead
        title={td('moves.title')}
        action={{ label: t('overview.showAll'), href: routes.finance }}
      />
      <MoveRows moves={moves} />
    </section>
  );
}

function MoveRows({ moves }: { moves: RecentMove[] }) {
  const { t } = useTranslation('dashboard');
  const fmt = useFormat();
  const { categories } = useCategories();
  const categoryName = useCategoryName();
  const today = utcDayFloor(Date.now() / 1000);
  const first = Math.min(...moves.map((m) => m.date));
  const { ready, convertAt } = useDatedMainConvert(first);
  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);

  const dayLabel = (date: number) => {
    const day = utcDayFloor(date);
    if (day === today) return t('common:time.today');
    if (day === today - 86400) return t('common:time.yesterday');
    return fmt.day(date, { day: 'numeric', month: 'numeric' });
  };

  return (
    <EntityRows>
      {moves.map((move) => {
        const bank = move.kind === 'bankIn' || move.kind === 'bankOut';
        const title = move.title || t(`moves.kind.${move.kind}`);
        const category = move.categoryId ? categoryById.get(move.categoryId) : undefined;
        const sub = bank
          ? [dayLabel(move.date), move.source, category ? categoryName(category) : null]
          : [dayLabel(move.date), t(`moves.kind.${move.kind}`), move.source];
        const Icon = MOVE_ICONS[move.kind];
        const sign = move.kind === 'bankIn' ? '+ ' : move.kind === 'bankOut' ? '− ' : '';
        return (
          <EntityRow
            key={move.id}
            chip={
              <RowChip tone={move.kind === 'bankIn' ? 'gain' : 'default'}>
                {Icon ? (
                  <Icon strokeWidth={1.75} />
                ) : (
                  initials(title) || <Landmark strokeWidth={1.75} />
                )}
              </RowChip>
            }
            title={title}
            sub={sub.filter(Boolean).join(' · ')}
            value={
              <ConvertedMoney
                prefix={sign}
                value={convertAt(move.amount, move.currency, move.date)}
                amount={move.amount}
                currency={move.currency}
                pending={!ready}
              />
            }
            valueTone={move.kind === 'bankIn' ? 'gain' : undefined}
          />
        );
      })}
    </EntityRows>
  );
}

/** A brand-new profile: the desktop's empty state with its first action (spec §8.3). */
function EmptyOverview() {
  const { t } = useTranslation('dashboard');
  const [, setLocation] = useLocation();
  return (
    <EmptyState
      icon={<Wallet />}
      title={t('empty.title')}
      description={t('empty.description')}
      className="px-5 py-10"
      action={
        <Button
          className="h-auto min-h-12 w-full whitespace-normal rounded-[10px] py-3 text-m-row"
          onClick={() => setLocation(routes.finance)}
        >
          <Landmark aria-hidden />
          {t('empty.addAccount')}
        </Button>
      }
    />
  );
}

/** While the metrics settle: the hero and the stats in outline. */
function OverviewSkeleton() {
  return (
    <div aria-hidden>
      <div className="mb-3.5 rounded-r4 border border-line bg-paper px-4 pb-3 pt-4 shadow-e1">
        <div className="skeleton h-3 w-24" />
        <div className="skeleton mt-3 h-9 w-56" />
        <div className="skeleton mt-3 h-3 w-40" />
        <div className="skeleton mt-4 h-[160px] w-full" />
      </div>
      <div className="mb-3.5 grid grid-cols-3 gap-2.5">
        {[0, 1, 2].map((i) => (
          <div key={i} className="rounded-r3 border border-line bg-paper px-3 py-2.5 shadow-e1">
            <div className="skeleton h-2.5 w-3/5" />
            <div className="skeleton mt-2.5 h-4 w-4/5" />
          </div>
        ))}
      </div>
    </div>
  );
}
