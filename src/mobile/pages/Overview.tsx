import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useLocation } from 'wouter';
import { Landmark, Settings, Wallet } from 'lucide-react';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { useDatedMainConvert } from '@/hooks/use-fx';
import { useGreeting } from '@/hooks/use-greeting';
import { useMilestones } from '@/hooks/use-milestones';
import { ASSET_CLASSES, useNetWorthOverview } from '@/hooks/use-net-worth-overview';
import { useNetWorthJumpTip } from '@/hooks/use-net-worth-jump-tip';
import { useRecentMoves, type RecentMove } from '@/hooks/use-recent-moves';
import { useFormat } from '@/lib/use-format';
import { utcDayFloor } from '@/utils/chart-axis';
import { splitMilestones } from '@/utils/milestones';
import { CHART_PERIODS, type ChartPeriod } from '@/utils/period';
import { Button } from '@/components/ui/button';
import {
  ScaledChartLegend,
  ScaledConversionNote,
  ScaledEmptyState,
} from '@/mobile/components/Scaled';
import { LoadError } from '@/mobile/components/LoadError';
import { AgendaRow } from '@/mobile/components/AgendaRow';
import { AllocationRing } from '@/mobile/components/AllocationRing';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { GettingStartedCard } from '@/mobile/components/GettingStartedCard';
import { Hero } from '@/mobile/components/Hero';
import { Money } from '@/mobile/components/Money';
import { SectionHead } from '@/mobile/components/SectionHead';
import { StatRow, type StatItem } from '@/mobile/components/StatRow';
import { UpkeepCard } from '@/mobile/components/UpkeepCard';
import { useApprox } from '@/mobile/hooks/use-approx';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useRefetchShown } from '@/mobile/hooks/use-refetch-shown';
import { useTransactionDay } from '@/mobile/hooks/use-transaction-day';
import { transactionSubFacts, transactionTitle } from '@/mobile/lib/transaction-title';
import { CategoryChip } from '@/mobile/components/cashflow/CategoryChip';
import { InstrumentChip } from '@/mobile/components/wealth/InstrumentChip';
import { exactMoneyOptions, SIGNED_WHOLE, WHOLE } from '@/mobile/lib/money-rule';
import { movePath } from '@/mobile/lib/move-path';
import { LockButton } from '@/mobile/shell/LockButton';
import { addAccountPath } from '@/mobile/lib/getting-started';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { statMoneyOptions } from '@/mobile/lib/stat-money';

/** Agenda rows shown on Přehled before "Zobrazit vše (N)". */
const AGENDA_ROWS = 3;
/** Rows of "Poslední pohyby". */
const MOVES = 5;

/**
 * Přehled tab root (spec §8.3, prototype frame 02): the greeting, the hero with the scrubbable
 * net-worth line and three stats — the app's main number first (audit D4) —, Údržba (only while
 * something is due), "Co vás čeká", Rozložení majetku and Poslední pohyby. Numbers come from the hooks the desktop dashboard uses.
 */
export default function Overview() {
  const { t } = useTranslation('mobile');
  const { t: td } = useTranslation('dashboard');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  // Money as text (the hero and the stats size themselves to it); `<Money>` where an element will do
  const moneyText = useMoneyText();
  const [period, setPeriod] = useState<ChartPeriod>('30D');
  // A pull from the top reads again what Přehled shows (spec §8.8)
  const refetchShown = useRefetchShown();

  const greeting = useGreeting();
  const asOf = td('asOf', {
    date: fmt.date(new Date(), { day: 'numeric', month: 'numeric', year: 'numeric' }),
  });
  const overview = useNetWorthOverview(period);
  const { reportCurrency, display, unknown, hasHistory, shortHistoryBase } = overview;
  const jumpTip = useNetWorthJumpTip(reportCurrency, moneyText);
  // The hero has loaded and the profile holds something: the sections under Údržba show
  const showContent =
    !overview.isCheckingPortfolio && !overview.metricsError && !overview.isEmptyPortfolio;

  const money = (value: number) => moneyText(value, reportCurrency, WHOLE);
  const signedMoney = (value: number) => (
    <Money value={value} currency={reportCurrency} options={SIGNED_WHOLE} />
  );
  const statMoney = (value: number, isUnknown: boolean) => {
    if (isUnknown) return '—';
    return moneyText(value, reportCurrency, statMoneyOptions(value));
  };

  // "za 30 dní", or "od 3. 10." when the history is shorter than the period
  const changeLabel =
    shortHistoryBase === null
      ? t(`dashboard:changePeriod.${period}`)
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
        label: t('stat.assets'),
        value: statMoney(display.assets, unknown.assets),
        note: unknown.assets
          ? undefined
          : hasHistory
            ? signedMoney(overview.assetsDelta)
            : t('overview.stats.noHistory'),
      },
      {
        label: t('stat.liabilities'),
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
        label: t('stat.freeCash'),
        value: statMoney(display.savings, false),
        note: t('stat.freeCashNote'),
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
      moneyText,
    ]
  );

  return (
    <Screen
      onRefresh={refetchShown}
      title={greeting}
      // Collapsed, the bar names the tab, as every other tab's does (C9)
      barTitle={t('tabs.overview')}
      large
      subtitle={asOf}
      leading={<LockButton />}
      trailing={
        // Nastavení: the cog, drawn as the nav bar's icon buttons are (`NavIconButton`)
        <Link
          href={routes.settings}
          aria-label={tc('nav.settings')}
          className="inline-flex size-11 flex-none items-center justify-center rounded-[12px] text-ink [-webkit-touch-callout:none] active:bg-well-2 [&_svg]:size-6"
        >
          <Settings strokeWidth={1.75} aria-hidden />
        </Link>
      }
    >
      {overview.isCheckingPortfolio ? (
        <OverviewSkeleton />
      ) : overview.metricsError ? (
        // A failed load is neither a new profile nor a net worth of 0 (audit C6)
        <LoadError message={t('loadError.overview')} />
      ) : overview.isEmptyPortfolio ? (
        <EmptyOverview />
      ) : (
        <>
          {/* Amounts the backend could not convert into the report currency (left out) */}
          <ScaledConversionNote
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
                    text: (
                      <>
                        {signedMoney(overview.netWorthDelta)} {changeLabel}
                      </>
                    ),
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
          >
            {/* The rings on the line name themselves, as under the desktop's chart (C11) */}
            {overview.jumps.length > 0 && overview.points.length > 1 && (
              <ScaledChartLegend
                items={[{ label: td('jumps.legend'), swatch: { kind: 'event', type: 'mark' } }]}
              />
            )}
          </Hero>
          <StatRow stats={stats} />
        </>
      )}
      {/* Under the net worth and its three stats (D4), only while something is due (it also asks
          for a first backup). Always mounted in this slot, whichever state the hero is in: it
          withholds its rows until its data has loaded, and an open sheet must not unmount when
          saving makes the portfolio metrics refetch. */}
      <UpkeepCard />
      {/* The sections with queries of their own do not wait for the hero (E4): each shows as soon
          as it has answered and hides itself when it has nothing; a new or failed profile has none */}
      {(showContent || overview.isCheckingPortfolio) && <AgendaSection />}
      {showContent && (
        <>
          <AllocationSection
            values={ASSET_CLASSES.map((cls) => ({
              key: cls,
              label: td(`cards.${cls}`),
              value: display[cls],
            }))}
            formatValue={money}
            formatTotal={(value) =>
              moneyText(value, reportCurrency, { compact: true, decimals: 1 })
            }
          />
        </>
      )}
      {(showContent || overview.isCheckingPortfolio) && <RecentMovesSection />}
      {showContent && <GettingStartedCard />}
    </Screen>
  );
}

/** "Co vás čeká" (spec §8.3 item 4): three agenda rows and "Zobrazit vše (N)"; hidden when empty. */
function AgendaSection() {
  const { t } = useTranslation('milestones');
  const { t: tm } = useTranslation('mobile');
  const { data: milestones = [] } = useMilestones();
  const { agenda } = splitMilestones(milestones, utcDayFloor(Date.now() / 1000));
  if (agenda.length === 0) return null;
  return (
    <section>
      <SectionHead
        title={t('card.title')}
        // The whole agenda is one tap away whenever there is one (C8)
        action={{ label: t('card.showAll', { count: agenda.length }), href: routes.agenda }}
      />
      <EntityRows>
        {agenda.slice(0, AGENDA_ROWS).map((m) => (
          <AgendaRow key={m.key} m={m} from={tm('tabs.overview')} />
        ))}
      </EntityRows>
    </section>
  );
}

/** Rozložení majetku (spec §8.3 item 5): the 110 pt ring and its legend; hidden without assets. */
function AllocationSection({
  values,
  formatValue,
  formatTotal,
}: {
  values: { key: string; label: string; value: number }[];
  formatValue: (value: number) => string;
  formatTotal: (value: number) => string;
}) {
  const { t } = useTranslation('mobile');
  const { t: td } = useTranslation('dashboard');
  const fmt = useFormat();
  if (!values.some((v) => v.value > 0)) return null;
  return (
    <section>
      <SectionHead
        title={td('allocation.title')}
        action={{ label: t('tabs.wealth'), href: routes.wealth }}
      />
      <div className="mb-3.5 rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 py-3.5 shadow-e1">
        <AllocationRing
          segments={values}
          // Its own short word (B3): the tab is Majetek, the tile Celková aktiva, the ring's centre Aktiva
          centerLabel={t('overview.ringCenter')}
          otherLabel={td('cards.other')}
          formatPercent={(ratio) => fmt.percent(ratio, 0)}
          formatValue={formatValue}
          formatTotal={formatTotal}
        />
      </div>
    </section>
  );
}

/**
 * Poslední pohyby (spec §8.3 item 6): five entity rows — a payment as every payment row (its
 * category's chip and `transactionTitle`), a trade with its instrument's logo or monogram, "Dnes · účet · kategorie", the exact amount in the move's own currency (the
 * account's, the trade's) and, when that is not the main currency, "≈" its worth at its day's rate
 * under it (README §13, "Money and currencies"). Only a bank credit is green with a plus; a debit
 * carries a minus; trades are neutral amounts whose direction is in the sub. A tap opens the
 * payment in its account (its sheet open) or the trade's position. Hidden when there is nothing
 * yet.
 */
function RecentMovesSection() {
  const { t: td } = useTranslation('dashboard');
  const { moves, isLoading } = useRecentMoves(MOVES);
  if (isLoading || moves.length === 0) return null;
  return (
    <section>
      {/* No "Zobrazit vše": the moves come from accounts, trades and assets, so no one list holds them */}
      <SectionHead title={td('moves.title')} />
      <MoveRows moves={moves} />
    </section>
  );
}

function MoveRows({ moves }: { moves: RecentMove[] }) {
  const { t } = useTranslation('dashboard');
  const { t: tm } = useTranslation('mobile');
  const moneyText = useMoneyText();
  const approx = useApprox();
  const { categories } = useCategories();
  const categoryName = useCategoryName();
  const first = Math.min(...moves.map((m) => m.date));
  const { ready, convertAt } = useDatedMainConvert(first);
  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);

  // A payment's day as every list of payments words it (C11)
  const dayLabel = useTransactionDay();

  return (
    <EntityRows>
      {moves.map((move) => {
        const bank = move.kind === 'bankIn' || move.kind === 'bankOut';
        // A payment is titled as on every list of payments, a trade by its instrument (C4)
        const title =
          (move.naming && transactionTitle(move.naming)) ||
          move.title ||
          t(`moves.kind.${move.kind}`);
        const category = move.categoryId ? categoryById.get(move.categoryId) : undefined;
        const sub = bank
          ? transactionSubFacts({
              day: dayLabel(move.date),
              account: move.source,
              category: category ? categoryName(category) : null,
            })
          : [dayLabel(move.date), t(`moves.kind.${move.kind}`), move.source];
        const sign = move.kind === 'bankIn' ? '+ ' : move.kind === 'bankOut' ? '− ' : '';
        return (
          <EntityRow
            key={move.id}
            // The avatar as everywhere (C3): a payment's category, a trade's logo or monogram
            chip={
              bank ? (
                <CategoryChip category={category} />
              ) : (
                <InstrumentChip
                  ticker={move.source}
                  type={
                    move.kind === 'cryptoBuy' || move.kind === 'cryptoSell' ? 'crypto' : 'stock'
                  }
                />
              )
            }
            title={title}
            sub={sub.filter(Boolean).join(' · ')}
            // The move's own amount; its worth in the main currency is the "≈" under it
            value={`${sign}${moneyText(
              move.amount,
              move.currency,
              exactMoneyOptions(move.amount, move.currency)
            )}`}
            valueSub={approx(move.amount, move.currency, {
              converted: convertAt(move.amount, move.currency, move.date),
              pending: !ready,
            })}
            valueTone={move.kind === 'bankIn' ? 'gain' : undefined}
            // A payment opens in its account with its sheet, a trade on its position (C11)
            href={movePath(move) ?? undefined}
            from={tm('tabs.overview')}
          />
        );
      })}
    </EntityRows>
  );
}

/**
 * A brand-new profile: the desktop's empty state with its first action, which opens the account
 * form on Finance at once (`?add=account`), and the Začínáme checklist under it (spec §8.3).
 */
function EmptyOverview() {
  const { t } = useTranslation('dashboard');
  const { t: tm } = useTranslation('mobile');
  const [, setLocation] = useLocation();
  return (
    <>
      <ScaledEmptyState
        icon={<Wallet />}
        title={t('empty.title')}
        // The desktop's sentence says "on this computer"
        description={tm('overview.emptyDescription')}
        className="px-5 py-10"
        action={
          <Button
            className="h-auto min-h-12 w-full whitespace-normal rounded-[10px] py-3 text-m-row"
            onClick={() => setLocation(addAccountPath)}
          >
            <Landmark aria-hidden />
            {t('empty.addAccount')}
          </Button>
        }
      />
      <GettingStartedCard />
    </>
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
