import { useMemo, useState } from 'react';
import { Link, useLocation, useRoute } from 'wouter';
import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { ArrowUp, Calculator, Ellipsis, Link2, Pencil, Trash2, Unlink } from 'lucide-react';
import type {
  AssetValuation,
  InsurancePolicy,
  Loan,
  LoanSchedule,
  RealEstate,
  RealEstateOneTimeCost,
} from '@shared/schema';
import { convertToCzK, type CurrencyCode } from '@shared/currencies';
import { annualizeAmount, todayUtcDay } from '@shared/calculations';
import { insuranceApi, loansApi, portfolioApi, realEstateApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { formatAmountWithCode } from '@/utils/format-amount';
import type { ChartEvent, EventCluster } from '@/utils/chart-scale';
import { dailyValuations, valuationTrace } from '@/utils/valuation-trace';
import { useShellPage } from '@/components/shell/shell-context';
import { BackLink, PageHead } from '@/components/shell/PageHead';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { Stat, Stats } from '@/components/common/Stat';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { MoonyLineChart } from '@/components/charts/MoonyLineChart';
import { ChartLegend, type LegendItem } from '@/components/charts/ChartLegend';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Segmented } from '@/components/ui/segmented';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
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
import { AddRealEstateModal } from '@/components/real-estate/AddRealEstateModal';
import { OneTimeCostModal } from '@/components/real-estate/OneTimeCostModal';
import { AddRecurringCostModal } from '@/components/real-estate/AddRecurringCostModal';
import { PhotoTimelineGallery } from '@/components/real-estate/PhotoTimelineGallery';
import { RealEstateDocuments } from '@/components/real-estate/RealEstateDocuments';
import { RealEstateNotes } from '@/components/real-estate/RealEstateNotes';
import { LinkExistingDialog } from '@/components/real-estate/LinkExistingDialog';
import { RevalueDialog } from '@/components/valuations/RevalueDialog';
import { cn } from '@/lib/utils';

type Period = '12' | '36' | 'all';
const DAY = 86_400;

/** A mark on the value trace: the purchase (`buy`) or an estimate (`mark`). */
interface TraceEvent extends ChartEvent {
  kind: 'purchase' | 'estimate';
  /** In the display currency. */
  value: number;
  note?: string | null;
  /** The earliest estimate: the one that starts the log. */
  first?: boolean;
}

const czk = (amount: string | number | null | undefined, currency: string | null | undefined) =>
  convertToCzK(Number(amount) || 0, (currency || 'CZK') as CurrencyCode);

/** Yearly factor of a recurring real-estate cost (`monthly` / `quarterly` / `yearly`). */
function yearlyFactor(frequency: string | undefined): number {
  switch ((frequency || '').toLowerCase()) {
    case 'monthly':
      return 12;
    case 'quarterly':
      return 4;
    default:
      return 1;
  }
}

/**
 * Property detail (design system §7 Detail, prototype real-estate-detail.html):
 * valuation trace (the purchase when its day is known, every estimate, today)
 * with the purchase and the revaluations as events and the purchase price as
 * the reference, four stats (gross yield, net cashflow, principal repaid by the
 * tenant, return on equity), finance tab with linked loans and policies and
 * the annual balance, then costs, gallery, documents and notes.
 */
export default function RealEstateDetail() {
  const [, params] = useRoute('/real-estate/:id');
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const id = params?.id;
  const { formatCurrency, convert, currencyCode } = useCurrency();
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const today = todayUtcDay();
  const nowSec = Math.floor(Date.now() / 1000);

  const [period, setPeriod] = useState<Period>('all');
  const [editOpen, setEditOpen] = useState(false);
  const [revalueOpen, setRevalueOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [pendingDeleteCost, setPendingDeleteCost] = useState<RealEstateOneTimeCost | null>(null);
  const [linkLoanOpen, setLinkLoanOpen] = useState(false);
  const [linkInsuranceOpen, setLinkInsuranceOpen] = useState(false);

  const { data: realEstate, isLoading } = useQuery<RealEstate | null>({
    queryKey: ['real-estate', id],
    queryFn: () => realEstateApi.get(id!),
    enabled: !!id,
  });
  const { data: valuations = [] } = useQuery<AssetValuation[]>({
    queryKey: ['real-estate-valuations', id],
    queryFn: () => realEstateApi.getValuations(id!),
    enabled: !!id,
  });
  const { data: oneTimeCosts = [] } = useQuery<RealEstateOneTimeCost[]>({
    queryKey: ['real-estate-costs', id],
    queryFn: () => realEstateApi.getCosts(id!),
    enabled: !!id,
  });
  const { data: linkedLoans = [] } = useQuery<Loan[]>({
    queryKey: ['real-estate-loans', id],
    queryFn: () => realEstateApi.getLoans(id!),
    enabled: !!id,
  });
  const { data: linkedInsurances = [] } = useQuery<InsurancePolicy[]>({
    queryKey: ['real-estate-insurances', id],
    queryFn: () => realEstateApi.getInsurances(id!),
    enabled: !!id,
  });
  // Schedules split the next year's payments into interest and principal
  const scheduleQueries = useQueries({
    queries: linkedLoans.map((loan) => ({
      queryKey: ['loan-schedule', loan.id],
      queryFn: () => loansApi.getSchedule(loan.id),
      staleTime: 60 * 1000,
    })),
  });
  const schedules = scheduleQueries.map((q) => q.data as LoanSchedule | undefined);

  const invalidateProperty = async () => {
    queryClient.invalidateQueries({ queryKey: ['real-estate'] });
    queryClient.invalidateQueries({ queryKey: ['real-estate', id] });
    queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
    queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
    try {
      await portfolioApi.recordSnapshot();
      queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
    } catch (error) {
      console.error('Failed to record portfolio snapshot:', error);
    }
  };
  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  const deleteMutation = useMutation({
    mutationFn: () => realEstateApi.delete(id!),
    onSuccess: async () => {
      await invalidateProperty();
      toast(t('toast.deleted'));
      setLocation('/real-estate');
    },
    onError,
  });
  const deleteCostMutation = useMutation({
    mutationFn: (costId: string) => realEstateApi.deleteCost(costId),
    onSuccess: () => {
      setPendingDeleteCost(null);
      queryClient.invalidateQueries({ queryKey: ['real-estate-costs', id] });
      toast(t('toast.costDeleted'));
    },
    onError,
  });

  const invalidateLinks = () => {
    queryClient.invalidateQueries({ queryKey: ['real-estate-insurances', id] });
    queryClient.invalidateQueries({ queryKey: ['real-estate-loans', id] });
    queryClient.invalidateQueries({ queryKey: ['available-insurances'] });
    queryClient.invalidateQueries({ queryKey: ['available-loans'] });
    queryClient.invalidateQueries({ queryKey: ['loans'] });
    queryClient.invalidateQueries({ queryKey: ['insurance'] });
    queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
  };
  const unlinkLoanMutation = useMutation({
    mutationFn: (loanId: string) => realEstateApi.unlinkLoan(id!, loanId),
    onSuccess: () => {
      invalidateLinks();
      toast(t('toast.loanUnlinked'));
    },
    onError,
  });
  const unlinkInsuranceMutation = useMutation({
    mutationFn: (insuranceId: string) => realEstateApi.unlinkInsurance(id!, insuranceId),
    onSuccess: () => {
      invalidateLinks();
      toast(t('toast.insuranceUnlinked'));
    },
    onError,
  });
  const { data: allLoans, isLoading: loansLoading } = useQuery<Loan[]>({
    queryKey: ['loans'],
    queryFn: () => loansApi.getAll(),
    enabled: linkLoanOpen,
  });
  const { data: allInsurances, isLoading: insurancesLoading } = useQuery<InsurancePolicy[]>({
    queryKey: ['insurance'],
    queryFn: () => insuranceApi.getAll(),
    enabled: linkInsuranceOpen,
  });
  const linkedLoanIds = new Set(linkedLoans.map((l) => l.id));
  const linkedInsuranceIds = new Set(linkedInsurances.map((p) => p.id));
  const loanCandidates = (allLoans ?? [])
    .filter((l) => !linkedLoanIds.has(l.id))
    .map((l) => ({ id: l.id, label: l.name, meta: fmt.money(Number(l.principal), l.currency) }));
  const insuranceCandidates = (allInsurances ?? [])
    .filter((p) => !linkedInsuranceIds.has(p.id))
    .map((p) => ({ id: p.id, label: p.policyName, meta: `${p.provider} · ${p.type}` }));
  const linkLoanMutation = useMutation({
    mutationFn: (loanId: string) => realEstateApi.linkLoan(id!, loanId),
    onSuccess: () => {
      invalidateLinks();
      setLinkLoanOpen(false);
      toast(t('toast.loanLinked'));
    },
    onError,
  });
  const linkInsuranceMutation = useMutation({
    mutationFn: (insuranceId: string) => realEstateApi.linkInsurance(id!, insuranceId),
    onSuccess: () => {
      invalidateLinks();
      setLinkInsuranceOpen(false);
      toast(t('toast.insuranceLinked'));
    },
    onError,
  });

  // ---- derived numbers (CZK, then formatCurrency converts to the display currency) ----
  const valueCzk = czk(realEstate?.marketPrice, realEstate?.marketPriceCurrency);
  const purchaseCzk = czk(realEstate?.purchasePrice, realEstate?.purchasePriceCurrency);
  const appreciation = valueCzk - purchaseCzk;
  const rentMonthlyCzk = czk(realEstate?.monthlyRent, realEstate?.monthlyRentCurrency);
  const rentYearCzk = rentMonthlyCzk * 12;
  const loansCzk = linkedLoans.reduce((s, l) => s + czk(l.outstandingBalance, l.currency), 0);
  const equityCzk = valueCzk - loansCzk;
  const ltv = valueCzk > 0 ? loansCzk / valueCzk : 0;
  const recurringYearCzk = (realEstate?.recurringCosts ?? []).reduce(
    (s, c) => s + czk(Number(c.amount) * yearlyFactor(c.frequency), c.currency),
    0
  );
  const insuranceYearCzk = linkedInsurances.reduce(
    (s, p) =>
      s +
      czk(
        annualizeAmount(Number(p.regularPayment) || 0, p.paymentFrequency),
        p.regularPaymentCurrency
      ),
    0
  );
  const paymentsYearCzk = linkedLoans.reduce(
    (s, l) => s + czk(Number(l.monthlyPayment) * 12, l.currency),
    0
  );
  // Next 12 payments split by the schedules; a missing schedule falls back to rate × balance
  const { interestYearCzk, principalYearCzk } = linkedLoans.reduce(
    (acc, loan, i) => {
      const schedule = schedules[i];
      const horizon = today + 365 * DAY;
      if (schedule) {
        const next = schedule.rows.filter((r) => r.dueDay > today && r.dueDay <= horizon);
        acc.interestYearCzk += czk(
          next.reduce((s, r) => s + Number(r.interest), 0),
          loan.currency
        );
        acc.principalYearCzk += czk(
          next.reduce((s, r) => s + Number(r.principalPart), 0),
          loan.currency
        );
      } else {
        const interest = (Number(loan.outstandingBalance) * (Number(loan.interestRate) || 0)) / 100;
        acc.interestYearCzk += czk(interest, loan.currency);
        acc.principalYearCzk += czk(Number(loan.monthlyPayment) * 12 - interest, loan.currency);
      }
      return acc;
    },
    { interestYearCzk: 0, principalYearCzk: 0 }
  );
  const netIncomeCzk = rentYearCzk - recurringYearCzk - insuranceYearCzk - interestYearCzk;
  const cashflowCzk = netIncomeCzk - principalYearCzk;
  const grossYield = valueCzk > 0 ? rentYearCzk / valueCzk : 0;
  const cashOnCash = equityCzk > 0 ? netIncomeCzk / equityCzk : 0;
  const oneTimeTotalCzk = oneTimeCosts.reduce((s, c) => s + czk(c.amount, c.currency), 0);
  const lastOneTime = oneTimeCosts.reduce<RealEstateOneTimeCost | null>(
    (max, c) => (max === null || c.date > max.date ? c : max),
    null
  );

  // ---- valuation trace ----
  const purchaseDisplay = convert(purchaseCzk, 'CZK', currencyCode);
  // The purchase is on the trace only when both its day and its price are known.
  const purchaseDay = realEstate?.purchaseDate ?? null;
  const purchaseKnown = purchaseDay !== null && purchaseDisplay > 0;
  // One estimate per day (the newest one), in the display currency.
  const estimates = useMemo(
    () =>
      dailyValuations(
        valuations.map((v) => ({
          id: v.id,
          t: v.valuedAt,
          value: convert(Number(v.value) || 0, (v.currency || 'CZK') as CurrencyCode, currencyCode),
          createdAt: v.createdAt,
          note: v.note,
        }))
      ),
    [valuations, convert, currencyCode]
  );
  // The window opens on a UTC day (ADR 0008).
  const windowStart =
    period === 'all' ? undefined : today - (period === '12' ? 365 : 3 * 365) * DAY;
  const points = useMemo(
    () =>
      valuationTrace({
        valuations: estimates,
        purchase: purchaseKnown ? { t: purchaseDay, value: purchaseDisplay } : null,
        now: nowSec,
        start: windowStart,
      }),
    [estimates, purchaseKnown, purchaseDay, purchaseDisplay, nowSec, windowStart]
  );
  const events = useMemo<TraceEvent[]>(() => {
    const marks = estimates
      .map<TraceEvent>((e, i) => ({
        id: e.id,
        t: e.t,
        type: 'mark',
        kind: 'estimate',
        value: e.value,
        note: e.note,
        first: i === 0,
      }))
      // The purchase mark wins where an estimate falls on the same day: they would overlap.
      .filter((e) => !(purchaseKnown && e.t === purchaseDay));
    return purchaseKnown
      ? [
          {
            id: 'purchase',
            t: purchaseDay,
            type: 'buy',
            kind: 'purchase',
            value: purchaseDisplay,
          },
          ...marks,
        ]
      : marks;
  }, [estimates, purchaseKnown, purchaseDay, purchaseDisplay]);
  const eventTip = (cluster: EventCluster<TraceEvent>) => {
    const e = cluster.events[0];
    const label =
      e.kind === 'purchase'
        ? t('detail.chart.bought')
        : e.first
          ? t('detail.chart.first')
          : t('detail.chart.revaluation');
    return {
      title: `${label} · ${fmt.money(e.value, currencyCode, { decimals: 0 })}`,
      lines: [fmt.day(e.t), e.note ?? ''].filter(Boolean),
    };
  };
  const latestEstimate = estimates[estimates.length - 1];

  useShellPage({
    crumb: realEstate?.name,
    status: latestEstimate
      ? {
          text: t('detail.statusValuation', { date: fmt.day(latestEstimate.t) }),
          tone: 'neutral',
        }
      : undefined,
  });

  if (isLoading) {
    return (
      <>
        <BackLink href="/real-estate">{t('detail.backToList')}</BackLink>
        <div className="mb-[26px]">
          <div className="skeleton h-2.5 w-32" />
          <div className="skeleton mt-3 h-9 w-72" />
          <div className="skeleton mt-3 h-3 w-96" />
        </div>
        <Card className="px-6 pb-4 pt-6">
          <div className="skeleton h-3 w-24" />
          <div className="skeleton mt-3 h-10 w-56" />
          <div className="skeleton mt-6 h-[210px] w-full" />
        </Card>
      </>
    );
  }
  if (!realEstate) {
    return (
      <>
        <BackLink href="/real-estate">{t('detail.backToList')}</BackLink>
        <p className="mt-8 text-center text-ink-3">{tc('status.error')}</p>
      </>
    );
  }

  const kv = (label: string, value: string, tone?: 'gain' | 'loss' | 'total') => (
    <div
      className={cn(
        'grid grid-cols-[1fr_auto] gap-x-4 border-b border-line-soft py-[9px] text-table text-ink-3 last:border-0',
        tone === 'total' && 'mt-0.5 border-t border-t-line-strong font-700 text-ink'
      )}
    >
      <span>{label}</span>
      <b
        className={cn(
          'font-600 text-ink num',
          tone === 'gain' && 'text-gain',
          tone === 'loss' && 'text-loss',
          tone === 'total' && 'font-700'
        )}
      >
        {value}
      </b>
    </div>
  );
  const signed = (v: number) =>
    fmt.money(convert(v, 'CZK', currencyCode as CurrencyCode), currencyCode, {
      signed: true,
      decimals: 0,
    });
  const sub = (text: string) => (
    <small className="mt-[3px] block text-micro font-500 text-ink-4">{text}</small>
  );
  const legendItems: LegendItem[] = [
    { label: t('detail.chart.value'), swatch: { kind: 'line' } },
    { label: t('detail.chart.purchase'), swatch: { kind: 'dash' } },
  ];
  if (purchaseKnown) {
    legendItems.push({ label: t('detail.chart.bought'), swatch: { kind: 'event', type: 'buy' } });
  }
  legendItems.push({
    label: t('detail.chart.revaluation'),
    swatch: { kind: 'event', type: 'mark' },
  });
  const mortgageText = linkedLoans
    .map((l) =>
      t('detail.mortgage', {
        name: l.name,
        amount: fmt.money(Number(l.outstandingBalance), l.currency, { decimals: 0 }),
      })
    )
    .join(' · ');

  return (
    <>
      <BackLink href="/real-estate">{t('detail.backToList')}</BackLink>
      <PageHead
        eyebrow={[
          t(`types.${realEstate.type}`),
          realEstate.address,
          purchaseDay !== null
            ? t('detail.boughtOnFor', {
                date: fmt.day(purchaseDay),
                amount: formatCurrency(purchaseCzk),
              })
            : t('detail.boughtFor', { amount: formatCurrency(purchaseCzk) }),
        ]}
        title={realEstate.name}
        description={[
          rentMonthlyCzk > 0
            ? t('detail.rented', { rent: formatCurrency(rentMonthlyCzk) })
            : t('detail.notRented'),
          mortgageText || null,
        ]
          .filter(Boolean)
          .join(' · ')}
        actions={
          <>
            <Button asChild variant="outline">
              <Link href={`/calculators/estate?property=${realEstate.id}`}>
                <Calculator />
                {t('detail.openCalculator')}
              </Link>
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="icon" aria-label={tc('labels.moreActions')}>
                  <Ellipsis />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onSelect={() => setEditOpen(true)}>
                  <Pencil />
                  {t('detail.menu.edit')}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="danger" onSelect={() => setDeleteOpen(true)}>
                  <Trash2 />
                  {t('detail.menu.delete')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <Button onClick={() => setRevalueOpen(true)}>
              <ArrowUp />
              {t('detail.revalue')}
            </Button>
          </>
        }
      />

      <HeroCard
        aside={
          <Segmented
            value={period}
            onValueChange={setPeriod}
            options={[
              { value: '12', label: tc('periods.1Y') },
              { value: '36', label: tc('periods.3Y') },
              { value: 'all', label: tc('periods.All') },
            ]}
          />
        }
        chart={
          points.length > 0 ? (
            <>
              <MoonyLineChart<TraceEvent>
                className="-mx-1 mt-[18px]"
                points={points}
                height={210}
                formatValue={(v) => fmt.money(v, currencyCode, { decimals: 0 })}
                reference={{
                  value: purchaseDisplay,
                  label: t('detail.chart.purchase'),
                  color: 'var(--chart-cost)',
                }}
                renderTip={(p) => ({
                  title: fmt.money(p.value, currencyCode, { decimals: 0 }),
                  lines: [
                    `${fmt.day(p.t)} · ${t('detail.chart.tipValue', {
                      amount: '',
                      delta: fmt.money(p.value - purchaseDisplay, currencyCode, {
                        signed: true,
                        decimals: 0,
                      }),
                    }).replace(/^\s*·\s*/, '')}`,
                  ],
                })}
                events={events}
                renderEventTip={eventTip}
              />
              <ChartLegend
                items={legendItems}
                note={
                  estimates.length > 1
                    ? t('detail.chart.note', { count: estimates.length - 1 })
                    : t('detail.chart.hint')
                }
              />
            </>
          ) : undefined
        }
      >
        <HeroValue
          label={t('detail.heroValue')}
          value={formatCurrency(valueCzk)}
          tone={appreciation > 0 ? 'gain' : appreciation < 0 ? 'loss' : 'neutral'}
          delta={
            purchaseCzk > 0 ? `${appreciation >= 0 ? '↗' : '↘'} ${signed(appreciation)}` : undefined
          }
          deltaNote={
            purchaseCzk > 0
              ? `${fmt.percent(appreciation / purchaseCzk, 1, { signed: true })} ${t('detail.vsPurchase')}`
              : undefined
          }
        />
        <HeroValue
          compact
          label={t('detail.heroEquity')}
          value={formatCurrency(equityCzk)}
          deltaNote={
            loansCzk > 0
              ? t('detail.equityNote', {
                  share: fmt.percent(valueCzk > 0 ? equityCzk / valueCzk : 0, 0),
                  ltv: fmt.percent(ltv, 0),
                })
              : t('detail.equityNoLoan')
          }
        />
      </HeroCard>

      <Stats className="mt-[17px]">
        <Stat
          label={t('detail.stats.grossYield')}
          value={rentYearCzk > 0 ? fmt.percent(grossYield, 1) : '—'}
          note={
            rentYearCzk > 0
              ? t('detail.stats.grossYieldNote', {
                  rent: formatCurrency(rentYearCzk),
                  purchaseYield: fmt.percent(purchaseCzk > 0 ? rentYearCzk / purchaseCzk : 0, 1),
                })
              : t('detail.stats.noRentNote')
          }
        />
        <Stat
          label={t('detail.stats.cashflow')}
          value={signed(cashflowCzk)}
          tone={cashflowCzk > 0 ? 'gain' : cashflowCzk < 0 ? 'loss' : 'neutral'}
          noteTone="neutral"
          note={
            paymentsYearCzk > 0
              ? t('detail.stats.cashflowNote', { payments: formatCurrency(paymentsYearCzk) })
              : t('detail.stats.cashflowNoLoanNote')
          }
        />
        <Stat
          label={t('detail.stats.principal')}
          value={principalYearCzk > 0 ? signed(principalYearCzk) : '—'}
          tone={principalYearCzk > 0 ? 'gain' : 'neutral'}
          noteTone="neutral"
          note={
            principalYearCzk > 0
              ? rentYearCzk > 0
                ? t('detail.stats.principalNote', {
                    share: fmt.percent(
                      paymentsYearCzk > 0 ? Math.min(1, rentYearCzk / paymentsYearCzk) : 0,
                      0
                    ),
                  })
                : t('detail.stats.principalNoRentNote')
              : t('detail.stats.principalNoLoan')
          }
        />
        <Stat
          label={t('detail.stats.cashOnCash')}
          value={equityCzk > 0 && rentYearCzk > 0 ? fmt.percent(cashOnCash, 1) : '—'}
          note={t('detail.stats.cashOnCashNote', {
            income: signed(netIncomeCzk),
            equity: formatCurrency(equityCzk),
          })}
        />
      </Stats>

      <Tabs defaultValue="financials">
        <TabsList variant="underline">
          <TabsTrigger value="financials">{t('detail.tabs.financials')}</TabsTrigger>
          <TabsTrigger value="costs">{t('detail.tabs.costs')}</TabsTrigger>
          <TabsTrigger value="history">
            {t('detail.tabs.history')}
            {oneTimeCosts.length > 0 && <span className="text-ink-4">{oneTimeCosts.length}</span>}
          </TabsTrigger>
          <TabsTrigger value="gallery">{t('detail.tabs.gallery')}</TabsTrigger>
          <TabsTrigger value="documents">{t('detail.tabs.documents')}</TabsTrigger>
          <TabsTrigger value="notes">{t('detail.tabs.notes')}</TabsTrigger>
        </TabsList>

        <TabsContent value="financials" className="mt-0">
          <div className="grid grid-cols-[1.4fr_0.6fr] items-start gap-[18px]">
            <div className="grid gap-[14px]">
              <Card variant="table">
                <CardHeader>
                  <div>
                    <CardTitle>{t('detail.loans.title')}</CardTitle>
                    <CardDescription>{t('detail.loans.sub')}</CardDescription>
                  </div>
                  <Button variant="outline" size="sm" onClick={() => setLinkLoanOpen(true)}>
                    <Link2 />
                    {t('detail.loans.link')}
                  </Button>
                </CardHeader>
                {linkedLoans.length === 0 ? (
                  <p className="border-t border-line px-[14px] py-8 text-center text-table text-ink-3">
                    {t('detail.loans.empty')}
                  </p>
                ) : (
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>{t('detail.loans.loan')}</TableHead>
                        <TableHead className="text-right">
                          {t('detail.loans.outstanding')}
                        </TableHead>
                        <TableHead className="text-right">{t('detail.loans.rate')}</TableHead>
                        <TableHead className="text-right">{t('detail.loans.payment')}</TableHead>
                        <TableHead className="w-10">
                          <span className="sr-only">{tc('labels.actions')}</span>
                        </TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {linkedLoans.map((loan) => {
                        const principal = Number(loan.principal) || 0;
                        const outstanding = Number(loan.outstandingBalance) || 0;
                        return (
                          <TableRow
                            key={loan.id}
                            className="h-[54px] cursor-pointer"
                            onClick={() => setLocation(`/loans/${loan.id}`)}
                          >
                            <TableCell>
                              <div className="flex min-w-0 items-center gap-[10px]">
                                <AssetLogo ticker={loan.name} type="stock" variant="soft" />
                                <div className="min-w-0">
                                  <b className="block truncate text-table font-650 text-ink">
                                    {loan.name}
                                  </b>
                                  {sub(
                                    loan.endDate
                                      ? t('detail.loans.principalUntil', {
                                          amount: fmt.money(principal, loan.currency, {
                                            decimals: 0,
                                          }),
                                          date: fmt.month(new Date(loan.endDate * 1000)),
                                        })
                                      : t('detail.loans.principalOnly', {
                                          amount: fmt.money(principal, loan.currency, {
                                            decimals: 0,
                                          }),
                                        })
                                  )}
                                </div>
                              </div>
                            </TableCell>
                            <TableCell className="text-right font-650 text-ink num">
                              {fmt.money(outstanding, loan.currency, { decimals: 0 })}
                              {principal > 0 &&
                                sub(
                                  t('detail.loans.repaid', {
                                    percent: fmt.percent(1 - outstanding / principal, 0),
                                  })
                                )}
                            </TableCell>
                            <TableCell className="text-right num">
                              {fmt.percent((Number(loan.interestRate) || 0) / 100, 2)}
                              {loan.interestRateValidityDate &&
                                sub(
                                  t('detail.loans.fixedUntil', {
                                    date: fmt.month(new Date(loan.interestRateValidityDate * 1000)),
                                  })
                                )}
                            </TableCell>
                            <TableCell className="text-right num">
                              {fmt.money(Number(loan.monthlyPayment) || 0, loan.currency, {
                                decimals: 0,
                              })}
                              {sub(t('detail.loans.monthly'))}
                            </TableCell>
                            <TableCell className="text-right">
                              <div data-row-actions onClick={(e) => e.stopPropagation()}>
                                <Button
                                  variant="ghost"
                                  size="icon-sm"
                                  aria-label={t('detail.unlinkItem', { name: loan.name })}
                                  title={t('detail.loans.unlink')}
                                  onClick={() => unlinkLoanMutation.mutate(loan.id)}
                                >
                                  <Unlink />
                                </Button>
                              </div>
                            </TableCell>
                          </TableRow>
                        );
                      })}
                    </TableBody>
                  </Table>
                )}
              </Card>

              <Card variant="table">
                <CardHeader>
                  <div>
                    <CardTitle>{t('detail.insurances.title')}</CardTitle>
                    <CardDescription>{t('detail.insurances.sub')}</CardDescription>
                  </div>
                  <Button variant="outline" size="sm" onClick={() => setLinkInsuranceOpen(true)}>
                    <Link2 />
                    {t('detail.insurances.link')}
                  </Button>
                </CardHeader>
                {linkedInsurances.length === 0 ? (
                  <p className="border-t border-line px-[14px] py-8 text-center text-table text-ink-3">
                    {t('detail.insurances.empty')}
                  </p>
                ) : (
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>{t('detail.insurances.policy')}</TableHead>
                        <TableHead className="text-right">
                          {t('detail.insurances.premium')}
                        </TableHead>
                        <TableHead className="text-right">
                          {t('detail.insurances.yearly')}
                        </TableHead>
                        <TableHead className="w-10">
                          <span className="sr-only">{tc('labels.actions')}</span>
                        </TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {linkedInsurances.map((policy) => {
                        const premium = Number(policy.regularPayment) || 0;
                        return (
                          <TableRow
                            key={policy.id}
                            className="h-[54px] cursor-pointer"
                            onClick={() => setLocation(`/insurance/${policy.id}`)}
                          >
                            <TableCell>
                              <div className="flex min-w-0 items-center gap-[10px]">
                                <AssetLogo ticker={policy.provider} type="stock" variant="soft" />
                                <div className="min-w-0">
                                  <b className="block truncate text-table font-650 text-ink">
                                    {policy.policyName}
                                  </b>
                                  {sub(`${policy.provider} · ${policy.type}`)}
                                </div>
                              </div>
                            </TableCell>
                            <TableCell className="text-right font-650 text-ink num">
                              {fmt.money(premium, policy.regularPaymentCurrency, { decimals: 0 })}
                              {sub(policy.paymentFrequency)}
                            </TableCell>
                            <TableCell className="text-right num">
                              {fmt.money(
                                annualizeAmount(premium, policy.paymentFrequency),
                                policy.regularPaymentCurrency,
                                { decimals: 0 }
                              )}
                            </TableCell>
                            <TableCell className="text-right">
                              <div data-row-actions onClick={(e) => e.stopPropagation()}>
                                <Button
                                  variant="ghost"
                                  size="icon-sm"
                                  aria-label={t('detail.unlinkItem', { name: policy.policyName })}
                                  title={t('detail.insurances.unlink')}
                                  onClick={() => unlinkInsuranceMutation.mutate(policy.id)}
                                >
                                  <Unlink />
                                </Button>
                              </div>
                            </TableCell>
                          </TableRow>
                        );
                      })}
                    </TableBody>
                  </Table>
                )}
              </Card>
            </div>

            <aside className="grid gap-[14px]">
              <Card>
                <CardHeader className="pb-1.5">
                  <div>
                    <CardTitle className="text-h3">{t('detail.balance.title')}</CardTitle>
                    <CardDescription>{t('detail.balance.sub')}</CardDescription>
                  </div>
                </CardHeader>
                <CardContent>
                  {kv(
                    t('detail.balance.rent', { amount: formatCurrency(rentMonthlyCzk) }),
                    signed(rentYearCzk),
                    rentYearCzk > 0 ? 'gain' : undefined
                  )}
                  {kv(
                    t('detail.balance.recurring'),
                    signed(-recurringYearCzk),
                    recurringYearCzk > 0 ? 'loss' : undefined
                  )}
                  {kv(
                    t('detail.balance.insurance'),
                    signed(-insuranceYearCzk),
                    insuranceYearCzk > 0 ? 'loss' : undefined
                  )}
                  {kv(
                    t('detail.balance.interest'),
                    signed(-interestYearCzk),
                    interestYearCzk > 0 ? 'loss' : undefined
                  )}
                  {kv(t('detail.balance.netIncome'), signed(netIncomeCzk), 'total')}
                  {kv(
                    t('detail.balance.principal'),
                    signed(-principalYearCzk),
                    principalYearCzk > 0 ? 'loss' : undefined
                  )}
                  {kv(t('detail.balance.cashflow'), signed(cashflowCzk), 'total')}
                  <p className="mb-0 mt-3 text-caption leading-[1.5] text-ink-3">
                    {rentYearCzk === 0 && recurringYearCzk === 0 && paymentsYearCzk === 0
                      ? t('detail.balance.empty')
                      : cashflowCzk < 0
                        ? t('detail.balance.costs', {
                            amount: formatCurrency(Math.abs(cashflowCzk) / 12),
                            income: signed(netIncomeCzk),
                          })
                        : t('detail.balance.earns', {
                            amount: formatCurrency(cashflowCzk / 12),
                            income: signed(netIncomeCzk),
                          })}{' '}
                    <Link
                      href={`/calculators/estate?property=${realEstate.id}`}
                      className="font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
                    >
                      {t('detail.balance.calculator')}
                    </Link>
                  </p>
                </CardContent>
              </Card>
              <Card variant="flat">
                <CardContent className="pt-5">
                  <h3 className="m-0 text-h3 text-ink">{t('detail.oneTime.title')}</h3>
                  <p className="mb-0 mt-1.5 text-caption leading-[1.5] text-ink-3">
                    {oneTimeCosts.length > 0 && lastOneTime
                      ? t('detail.oneTime.text', {
                          count: oneTimeCosts.length,
                          total: formatCurrency(oneTimeTotalCzk),
                          last: `${lastOneTime.name} ${formatCurrency(czk(lastOneTime.amount, lastOneTime.currency))} (${fmt.day(lastOneTime.date)})`,
                          basis: formatCurrency(purchaseCzk + oneTimeTotalCzk),
                        })
                      : t('detail.oneTime.none', { basis: formatCurrency(purchaseCzk) })}
                  </p>
                </CardContent>
              </Card>
            </aside>
          </div>
        </TabsContent>

        <TabsContent value="costs" className="mt-0">
          <Card variant="table">
            <CardHeader>
              <div>
                <CardTitle>{t('detail.recurringCosts')}</CardTitle>
                <CardDescription>{t('detail.recurringCostsDesc')}</CardDescription>
              </div>
              <AddRecurringCostModal realEstate={realEstate} />
            </CardHeader>
            {(realEstate.recurringCosts ?? []).length === 0 ? (
              <p className="border-t border-line px-[14px] py-8 text-center text-table text-ink-3">
                {t('detail.noRecurringCosts')}
              </p>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t('detail.recurringTable.name')}</TableHead>
                    <TableHead>{t('detail.recurringTable.frequency')}</TableHead>
                    <TableHead className="text-right">
                      {t('detail.recurringTable.amount')}
                    </TableHead>
                    <TableHead className="text-right">
                      {t('detail.recurringTable.yearly')}
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {realEstate.recurringCosts.map((cost, i) => (
                    <TableRow key={`${cost.name}-${i}`} className="h-[50px]">
                      <TableCell className="font-650 text-ink">{cost.name}</TableCell>
                      <TableCell>
                        <Badge>
                          {t(`detail.frequency.${cost.frequency.toLowerCase()}`, {
                            defaultValue: cost.frequency,
                          })}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-right num">
                        {fmt.money(Number(cost.amount) || 0, cost.currency || 'CZK', {
                          decimals: 0,
                        })}
                      </TableCell>
                      <TableCell className="text-right font-650 text-ink num">
                        {formatCurrency(
                          czk(Number(cost.amount) * yearlyFactor(cost.frequency), cost.currency)
                        )}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
            <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
              <span>
                {t('detail.recurringTable.total', { amount: formatCurrency(recurringYearCzk) })}
              </span>
            </div>
          </Card>
        </TabsContent>

        <TabsContent value="history" className="mt-0">
          <Card variant="table">
            <CardHeader>
              <div>
                <CardTitle>{t('detail.oneTimeCosts')}</CardTitle>
                <CardDescription>{t('detail.oneTimeCostsDesc')}</CardDescription>
              </div>
              <OneTimeCostModal realEstateId={realEstate.id} />
            </CardHeader>
            {oneTimeCosts.length === 0 ? (
              <p className="border-t border-line px-[14px] py-8 text-center text-table text-ink-3">
                {t('detail.noOneTimeCosts')}
              </p>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{tc('labels.date')}</TableHead>
                    <TableHead>{tc('labels.name')}</TableHead>
                    <TableHead>{tc('labels.description')}</TableHead>
                    <TableHead className="text-right">{tc('labels.amount')}</TableHead>
                    <TableHead className="w-16">
                      <span className="sr-only">{tc('labels.actions')}</span>
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {[...oneTimeCosts]
                    .sort((a, b) => b.date - a.date)
                    .map((cost) => (
                      <TableRow key={cost.id} className="h-[50px]">
                        <TableCell>{fmt.day(cost.date)}</TableCell>
                        <TableCell className="font-650 text-ink">{cost.name}</TableCell>
                        <TableCell className="text-ink-3">{cost.description}</TableCell>
                        <TableCell className="text-right font-650 text-ink num">
                          {formatCurrency(czk(cost.amount, cost.currency))}
                        </TableCell>
                        <TableCell className="text-right">
                          <div data-row-actions className="inline-flex gap-0.5">
                            <OneTimeCostModal
                              realEstateId={realEstate.id}
                              cost={cost}
                              trigger={
                                <Button
                                  variant="ghost"
                                  size="icon-sm"
                                  aria-label={t('detail.editItem', { name: cost.name })}
                                >
                                  <Pencil />
                                </Button>
                              }
                            />
                            <Button
                              variant="ghost"
                              size="icon-sm"
                              aria-label={t('detail.deleteItem', { name: cost.name })}
                              onClick={() => setPendingDeleteCost(cost)}
                            >
                              <Trash2 />
                            </Button>
                          </div>
                        </TableCell>
                      </TableRow>
                    ))}
                </TableBody>
              </Table>
            )}
            <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
              <span>
                {t('detail.oneTime.title')} · {formatCurrency(oneTimeTotalCzk)}
              </span>
            </div>
          </Card>
        </TabsContent>

        <TabsContent value="gallery" className="mt-0">
          <PhotoTimelineGallery realEstateId={realEstate.id} />
        </TabsContent>
        <TabsContent value="documents" className="mt-0">
          <RealEstateDocuments realEstateId={realEstate.id} />
        </TabsContent>
        <TabsContent value="notes" className="mt-0">
          <RealEstateNotes realEstate={realEstate} />
        </TabsContent>
      </Tabs>

      <LinkExistingDialog
        open={linkLoanOpen}
        onOpenChange={setLinkLoanOpen}
        title={t('linkDialog.loanTitle')}
        description={t('linkDialog.loanDescription')}
        candidates={loanCandidates}
        emptyText={t('linkDialog.noLoans')}
        linkLabel={t('linkDialog.link')}
        isLoading={loansLoading}
        isPending={linkLoanMutation.isPending}
        onLink={(loanId) => linkLoanMutation.mutate(loanId)}
      />
      <LinkExistingDialog
        open={linkInsuranceOpen}
        onOpenChange={setLinkInsuranceOpen}
        title={t('linkDialog.insuranceTitle')}
        description={t('linkDialog.insuranceDescription')}
        candidates={insuranceCandidates}
        emptyText={t('linkDialog.noInsurances')}
        linkLabel={t('linkDialog.link')}
        isLoading={insurancesLoading}
        isPending={linkInsuranceMutation.isPending}
        onLink={(insuranceId) => linkInsuranceMutation.mutate(insuranceId)}
      />
      <AddRealEstateModal realEstate={realEstate} open={editOpen} onOpenChange={setEditOpen} />
      <RevalueDialog
        open={revalueOpen}
        onOpenChange={setRevalueOpen}
        kind="realEstate"
        asset={{
          id: realEstate.id,
          name: realEstate.name,
          currentValue: Number(realEstate.marketPrice) || 0,
          currency: realEstate.marketPriceCurrency || 'CZK',
        }}
      />
      <ConfirmDeleteDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.description', { name: realEstate.name })}
        onConfirm={() => deleteMutation.mutate()}
        isPending={deleteMutation.isPending}
        confirmLabel={t('detail.menu.delete')}
      />
      <ConfirmDeleteDialog
        open={pendingDeleteCost !== null}
        onOpenChange={(open) => {
          if (!open) setPendingDeleteCost(null);
        }}
        title={t('confirmDeleteCost.title')}
        description={
          pendingDeleteCost
            ? t('confirmDeleteCost.description', {
                name: pendingDeleteCost.name,
                date: fmt.day(pendingDeleteCost.date),
                amount: formatAmountWithCode(
                  Number(pendingDeleteCost.amount),
                  pendingDeleteCost.currency,
                  fmt.locale
                ),
              })
            : ''
        }
        onConfirm={() => pendingDeleteCost && deleteCostMutation.mutate(pendingDeleteCost.id)}
        isPending={deleteCostMutation.isPending}
      />
    </>
  );
}
