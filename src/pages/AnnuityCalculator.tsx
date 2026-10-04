import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown } from 'lucide-react';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useLoans } from '@/hooks/use-loans';
import { useShellPage } from '@/components/shell/shell-context';
import {
  aggregateByYear,
  generateAmortizationSchedule,
  getPeriodsPerYear,
  type PaymentPeriodicity,
} from '@/utils/annuity';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { Segmented } from '@/components/ui/segmented';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { Stat, Stats } from '@/components/common/Stat';
import { ExportButton } from '@/components/common/ExportButton';
import { MoonyBarChart, type BarMark } from '@/components/charts/MoonyBarChart';
import { MoonyLinesChart } from '@/components/charts/MoonyLinesChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { CalcField } from '@/components/calculators/CalcField';
import { parseCalcNumber } from '@/components/calculators/calc-number';

const FREQUENCIES: readonly PaymentPeriodicity[] = [
  'monthly',
  'quarterly',
  'semiAnnually',
  'annually',
];
const PREVIEW_ROWS = 10;
const DAY = 86_400;

/**
 * Annuity calculator (design system §7 Calculator, prototype
 * `annuity-calculator.html`): inputs in a flat card left, the payment hero
 * with the yearly principal/interest bars (or the interest share lines) and
 * milestones right, three stats, the schedule by years or by payments, and
 * the saving an optional extra yearly payment brings.
 */
export default function AnnuityCalculator() {
  const { t } = useTranslation('calculators');
  const { formatCurrencyRaw, currencyCode } = useCurrency();
  const fmt = useFormat();
  const { loans } = useLoans();

  const [amount, setAmount] = useState('3 500 000');
  const [years, setYears] = useState('25');
  const [rate, setRate] = useState('5,1');
  const [frequency, setFrequency] = useState<PaymentPeriodicity>('monthly');
  const [extra, setExtra] = useState('');
  const [view, setView] = useState<'years' | 'share'>('years');
  const [granularity, setGranularity] = useState<'years' | 'periods'>('years');
  const [showAll, setShowAll] = useState(false);

  useShellPage({ status: { text: t('annuity.status'), tone: 'neutral' } });

  const L = parseCalcNumber(amount);
  const Y = parseCalcNumber(years);
  const R = parseCalcNumber(rate);
  const X = parseCalcNumber(extra);
  const k = getPeriodsPerYear(frequency);

  const base = useMemo(
    () => generateAmortizationSchedule(L, R, Math.round(Y * k), k),
    [L, R, Y, k]
  );
  const withExtra = useMemo(
    () => (X > 0 ? generateAmortizationSchedule(L, R, Math.round(Y * k), k, X) : null),
    [L, R, Y, k, X]
  );
  const baseYears = useMemo(() => aggregateByYear(base.schedule), [base]);
  const extraYears = useMemo(
    () => (withExtra ? aggregateByYear(withExtra.schedule) : null),
    [withExtra]
  );

  const startYear = new Date().getFullYear();
  const yearLabel = (y: number) => String(startYear + y - 1);
  const money = (v: number) => formatCurrencyRaw(v);
  const yearsText = (n: number) => t('annuity.years', { count: n });

  const cross = baseYears.findIndex((y) => y.principal > y.interest);
  const half = baseYears.findIndex((y) => y.balance <= L / 2);
  const marks: BarMark[] = [];
  if (cross > 0) {
    marks.push({
      index: cross,
      title: t('annuity.milestones.cross'),
      lines: [t('annuity.milestones.year', { year: yearLabel(baseYears[cross].year) })],
    });
  }
  if (half > 0 && half !== cross) {
    marks.push({
      index: half,
      title: t('annuity.milestones.half'),
      lines: [t('annuity.milestones.year', { year: yearLabel(baseYears[half].year) })],
    });
  }

  const interestShare = base.totalPayments > 0 ? base.totalInterest / base.totalPayments : 0;
  const first = base.schedule[0];
  const last = base.schedule[base.schedule.length - 1];
  const saving = withExtra ? base.totalInterest - withExtra.totalInterest : 0;

  const loadLoan = (loan: (typeof loans)[number]) => {
    setAmount(fmt.number(parseFloat(loan.principal) || 0, { maximumFractionDigits: 0 }));
    setRate(fmt.number(parseFloat(loan.interestRate) || 0, { maximumFractionDigits: 2 }));
    if (loan.endDate) {
      const span = Math.max(1, Math.round((loan.endDate - loan.startDate) / DAY / 365.25));
      setYears(String(span));
    }
    setFrequency('monthly');
  };

  const exportCsv = async () => {
    const lines = [
      ['year', 'payments', 'principal', 'interest', 'balance'].join(';'),
      ...baseYears.map((y) =>
        [
          yearLabel(y.year),
          y.payment.toFixed(2),
          y.principal.toFixed(2),
          y.interest.toFixed(2),
          y.balance.toFixed(2),
        ].join(';')
      ),
    ];
    return { csv: lines.join('\n'), filename: 'annuity-schedule.csv', count: baseYears.length };
  };

  // Schedule rows for the table
  const rows: {
    label: string;
    payment: number;
    principal: number;
    interest: number;
    balance: number;
  }[] =
    granularity === 'years'
      ? baseYears.map((y) => ({
          label: yearLabel(y.year),
          payment: y.payment,
          principal: y.principal,
          interest: y.interest,
          balance: y.balance,
        }))
      : base.schedule.map((r) => ({
          label:
            frequency === 'monthly'
              ? `${yearLabel(r.year)} · ${fmt.month(new Date(Date.UTC(2000, r.month - 1, 1)), { month: 'short', year: undefined })}`
              : t('annuity.table.periodLabel', {
                  year: yearLabel(r.year),
                  n: Math.ceil(r.month / (12 / k)),
                }),
          payment: r.payment,
          principal: r.principalPayment,
          interest: r.interestPayment,
          balance: r.remainingBalance,
        }));
  const shownRows = showAll ? rows : rows.slice(0, PREVIEW_ROWS);
  const hasResult = base.schedule.length > 0;

  return (
    <>
      <PageHead
        eyebrow={t('annuity.eyebrow')}
        title={t('annuity.title')}
        description={t('annuity.subtitle')}
        actions={
          <>
            {loans.length > 0 && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button variant="outline">
                    {t('annuity.actions.loadLoan')}
                    <ChevronDown className="text-ink-4" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  {loans.map((loan) => (
                    <DropdownMenuItem key={loan.id} onSelect={() => loadLoan(loan)}>
                      {loan.name}
                      <span className="ml-auto pl-4 text-micro text-ink-4 num">
                        {fmt.money(parseFloat(loan.principal) || 0, loan.currency, { decimals: 0 })}
                      </span>
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            )}
            <ExportButton exportFn={exportCsv} label={t('annuity.actions.export')} />
          </>
        }
      />

      <div className="grid grid-cols-[340px_1fr] items-stretch gap-x-[18px] gap-y-[14px]">
        <Card variant="flat">
          <CardHeader className="pb-1">
            <CardTitle className="text-[16px]">{t('annuity.form.title')}</CardTitle>
          </CardHeader>
          <div className="grid gap-[14px] px-[21px] pb-5 pt-1">
            <CalcField
              id="amount"
              label={t('annuity.form.amount')}
              unit={currencyCode}
              value={amount}
              onChange={setAmount}
            />
            <div className="grid grid-cols-2 gap-3">
              <CalcField
                id="years"
                label={t('annuity.form.years')}
                unit={t('annuity.form.yearsUnit')}
                value={years}
                onChange={setYears}
              />
              <CalcField
                id="rate"
                label={t('annuity.form.rate')}
                unit="%"
                value={rate}
                onChange={setRate}
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="frequency">{t('annuity.form.frequency')}</Label>
              <Select
                value={frequency}
                onValueChange={(v) => setFrequency(v as PaymentPeriodicity)}
              >
                <SelectTrigger id="frequency">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {FREQUENCIES.map((f) => (
                    <SelectItem key={f} value={f}>
                      {t(`annuity.form.frequencies.${f}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <CalcField
              id="extra"
              label={t('annuity.form.extra')}
              optional={t('annuity.form.optional')}
              unit={currencyCode}
              value={extra}
              onChange={setExtra}
              hint={t('annuity.form.extraHint')}
            />
            <div className="rounded-[10px] bg-well px-[14px] py-3 text-table leading-[1.5] text-ink-2">
              {withExtra && extraYears ? (
                <>
                  <b className="font-650 text-ink">
                    {t('annuity.insight.extraYearly', { extra: money(X) })}
                  </b>{' '}
                  {t('annuity.insight.shortens', {
                    years: yearsText(baseYears.length - extraYears.length),
                  })}{' '}
                  <b className="font-650 text-ink">{money(saving)}</b>{' '}
                  {t('annuity.insight.onInterest')}
                </>
              ) : (
                t('annuity.insight.noExtra')
              )}
            </div>
          </div>
        </Card>

        <HeroCard
          className="flex flex-col"
          aside={
            <Segmented
              value={view}
              onValueChange={setView}
              options={[
                { value: 'years', label: t('annuity.hero.view.years') },
                { value: 'share', label: t('annuity.hero.view.share') },
              ]}
            />
          }
          chart={
            hasResult ? (
              <>
                {view === 'years' ? (
                  <MoonyBarChart
                    className="-mx-1 mt-4"
                    bars={baseYears.map((y) => ({
                      label: yearLabel(y.year).slice(2),
                      a: y.principal,
                      b: y.interest,
                    }))}
                    marks={marks}
                    names={[t('annuity.legend.principal'), t('annuity.legend.interest')]}
                    stacked
                    height={200}
                    formatValue={money}
                  />
                ) : (
                  <MoonyLinesChart
                    className="-mx-1 mt-4"
                    series={[
                      {
                        id: 'interest',
                        name: t('annuity.legend.interest'),
                        values: baseYears.map((y) =>
                          y.payment > 0 ? (y.interest / y.payment) * 100 : 0
                        ),
                      },
                      {
                        id: 'principal',
                        name: t('annuity.legend.principal'),
                        values: baseYears.map((y) =>
                          y.payment > 0 ? (y.principal / y.payment) * 100 : 0
                        ),
                      },
                    ]}
                    axisLabels={Array.from({ length: 5 }, (_, i) =>
                      yearLabel(baseYears[Math.round(((baseYears.length - 1) * i) / 4)]?.year ?? 1)
                    )}
                    height={200}
                    formatValue={(v) => fmt.percent(v / 100, 0)}
                    tipLabel={(i) => yearLabel(baseYears[i]?.year ?? 1)}
                  />
                )}
                <ChartLegend
                  items={[
                    {
                      label: t('annuity.legend.principal'),
                      swatch: { kind: 'block', color: 'var(--s1)' },
                    },
                    {
                      label: t('annuity.legend.interest'),
                      swatch: { kind: 'block', color: 'var(--s3)' },
                    },
                    { label: t('annuity.legend.milestone'), swatch: { kind: 'ring' } },
                  ]}
                />
              </>
            ) : (
              <p className="grid h-[200px] place-items-center text-table text-ink-3">
                {t('annuity.empty')}
              </p>
            )
          }
        >
          <HeroValue
            label={t(`annuity.hero.payment.${frequency}`)}
            value={hasResult ? money(base.periodicPayment) : '—'}
            delta={
              hasResult ? t('annuity.hero.total', { amount: money(base.totalPayments) }) : undefined
            }
            deltaNote={
              hasResult
                ? t('annuity.hero.interestShare', {
                    amount: money(base.totalInterest),
                    percent: fmt.percent(interestShare, 0),
                  })
                : undefined
            }
          />
        </HeroCard>

        <Stats columns={3} className="col-span-2 mb-0">
          <Stat
            label={t('annuity.stats.totalPaid')}
            value={hasResult ? money(base.totalPayments) : '—'}
            note={
              hasResult
                ? `${yearsText(baseYears.length)} · ${t('annuity.payments', { count: base.schedule.length })}`
                : undefined
            }
          />
          <Stat
            label={t('annuity.stats.totalInterest')}
            value={hasResult ? money(base.totalInterest) : '—'}
            note={
              hasResult && L > 0
                ? t('annuity.stats.totalInterestNote', {
                    percent: fmt.percent(base.totalInterest / L, 0),
                  })
                : undefined
            }
          />
          {withExtra && extraYears ? (
            <Stat
              label={t('annuity.stats.withExtra')}
              value={fmt.money(-saving, currencyCode, { signed: true, decimals: 0 })}
              tone="gain"
              noteTone="neutral"
              note={t('annuity.stats.withExtraNote', {
                short: yearsText(extraYears.length),
                long: yearsText(baseYears.length),
              })}
            />
          ) : (
            <Stat
              label={t('annuity.stats.firstShare')}
              value={first ? fmt.percent(first.interestPayment / first.payment, 0) : '—'}
              note={
                last
                  ? t('annuity.stats.firstShareNote', {
                      percent: fmt.percent(last.interestPayment / last.payment, 0),
                    })
                  : undefined
              }
            />
          )}
        </Stats>
      </div>

      {hasResult && (
        <Card variant="table" className="mt-[18px]">
          <CardHeader>
            <div>
              <CardTitle>{t('annuity.table.title')}</CardTitle>
              <CardDescription>
                {granularity === 'years'
                  ? t('annuity.table.subYears')
                  : t('annuity.table.subPeriods')}
              </CardDescription>
            </div>
            <Segmented
              value={granularity}
              onValueChange={(g) => {
                setGranularity(g);
                setShowAll(false);
              }}
              options={[
                { value: 'years', label: t('annuity.table.granularity.years') },
                { value: 'periods', label: t(`annuity.form.frequencies.${frequency}`) },
              ]}
            />
          </CardHeader>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-[18%]">
                  {granularity === 'years' ? t('annuity.table.year') : t('annuity.table.period')}
                </TableHead>
                <TableHead className="text-right">{t('annuity.table.payments')}</TableHead>
                <TableHead className="text-right">{t('annuity.table.principal')}</TableHead>
                <TableHead className="text-right">{t('annuity.table.interest')}</TableHead>
                <TableHead className="text-right">{t('annuity.table.balance')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {shownRows.map((r) => (
                <TableRow key={r.label} className="h-[42px]">
                  <TableCell className="font-650 text-ink">{r.label}</TableCell>
                  <TableCell className="text-right num">{money(r.payment)}</TableCell>
                  <TableCell className="text-right num">{money(r.principal)}</TableCell>
                  <TableCell className="text-right num">{money(r.interest)}</TableCell>
                  <TableCell className="text-right font-650 text-ink num">
                    {money(r.balance)}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
          <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
            <span>
              {t('annuity.table.footer', { shown: shownRows.length, total: rows.length })}
            </span>
            {rows.length > PREVIEW_ROWS && (
              <button
                type="button"
                className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
                onClick={() => setShowAll((v) => !v)}
              >
                {showAll ? t('annuity.table.showLess') : `${t('annuity.table.showAll')} →`}
              </button>
            )}
          </div>
        </Card>
      )}
    </>
  );
}
