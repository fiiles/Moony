import { useMemo, useState } from 'react';
import { useSearch } from 'wouter';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { ChevronDown } from 'lucide-react';
import type { RealEstate } from '@shared/schema';
import { realEstateApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useShellPage } from '@/components/shell/shell-context';
import { rentalResult, rentalSeries, type RentalInput } from '@/utils/rental';
import { horizonAxisLabels } from '@/utils/projection-milestones';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
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
import {
  MoonyBandChart,
  type BandMilestone,
  type BandPoint,
} from '@/components/charts/MoonyBandChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { CalcField, ComputedRow } from '@/components/calculators/CalcField';
import { parseCalcNumber } from '@/components/calculators/calc-number';
import { cn } from '@/lib/utils';

/** Monthly equivalent of a recurring cost of a property. */
function monthlyCost(cost: RealEstate['recurringCosts'][number]): number {
  const f = cost.frequency.toLowerCase();
  if (f.startsWith('year') || f.startsWith('annu')) return cost.amount / 12;
  if (f.startsWith('quarter')) return cost.amount / 3;
  if (f.startsWith('week')) return (cost.amount * 52) / 12;
  return cost.amount;
}

/**
 * Rental property calculator (design system §7 Calculator, prototype
 * `rental-calculator.html`): purchase, rent and projection inputs in flat
 * cards left; the equity hero with the ± 1 p.p. price-growth band, the
 * invested-capital reference and milestones, four stats and the yearly
 * summary right; the sale after N years under the inputs.
 */
/** Values a property prefills: purchase price, rent and the monthly recurring costs. */
function propertyDefaults(p: RealEstate, format: (v: number) => string) {
  const monthly = p.recurringCosts.reduce((s, c) => s + monthlyCost(c), 0);
  return {
    price: format(parseFloat(p.purchasePrice ?? '') || 0),
    rent: p.monthlyRent ? format(parseFloat(p.monthlyRent) || 0) : null,
    costs: monthly > 0 ? format(monthly) : null,
  };
}

/**
 * Entry: `?property=<id>` (the property detail's "Otevřít v kalkulačce pronájmu")
 * prefills the calculator once the property is loaded; the key remounts the
 * calculator with fresh state per property.
 */
export default function EstateCalculator() {
  const search = useSearch();
  const propertyId = new URLSearchParams(search).get('property');
  const { data: properties, isLoading } = useQuery({
    queryKey: ['real-estate'],
    queryFn: () => realEstateApi.getAll(),
    enabled: !!propertyId,
  });
  if (propertyId && isLoading) return <></>;
  const initial = propertyId ? properties?.find((p) => p.id === propertyId) : undefined;
  return <Calculator key={propertyId ?? 'blank'} initial={initial} />;
}

function Calculator({ initial }: { initial?: RealEstate }) {
  const { t } = useTranslation('calculators');
  const { formatCurrencyRaw, formatCurrencyShort, currencyCode } = useCurrency();
  const fmt = useFormat();
  const { data: properties = [] } = useQuery({
    queryKey: ['real-estate'],
    queryFn: () => realEstateApi.getAll(),
  });

  const initialValues = initial
    ? propertyDefaults(initial, (v) => fmt.number(v, { maximumFractionDigits: 0 }))
    : null;
  const [price, setPrice] = useState(initialValues?.price ?? '5 400 000');
  const [loan, setLoan] = useState('3 000 000');
  const [loanYears, setLoanYears] = useState('30');
  const [rate, setRate] = useState('4,1');
  const [rent, setRent] = useState(initialValues?.rent ?? '21 500');
  const [costs, setCosts] = useState(initialValues?.costs ?? '3 000');
  const [vacancy, setVacancy] = useState('0,5');
  const [rentGrowth, setRentGrowth] = useState('3');
  const [costsGrowth, setCostsGrowth] = useState('3');
  const [priceGrowth, setPriceGrowth] = useState('3');
  const [years, setYears] = useState('15');
  const [oneTime, setOneTime] = useState('250 000');

  useShellPage({ status: { text: t('estate.status'), tone: 'neutral' } });

  const input = useMemo<RentalInput>(
    () => ({
      price: parseCalcNumber(price),
      loan: parseCalcNumber(loan),
      loanYears: parseCalcNumber(loanYears),
      rate: parseCalcNumber(rate),
      monthlyRent: parseCalcNumber(rent),
      monthlyCosts: parseCalcNumber(costs),
      vacancyMonths: parseCalcNumber(vacancy),
      rentGrowth: parseCalcNumber(rentGrowth),
      costsGrowth: parseCalcNumber(costsGrowth),
      priceGrowth: parseCalcNumber(priceGrowth),
      years: Math.max(1, Math.round(parseCalcNumber(years))),
      oneTimeCosts: parseCalcNumber(oneTime),
    }),
    [
      price,
      loan,
      loanYears,
      rate,
      rent,
      costs,
      vacancy,
      rentGrowth,
      costsGrowth,
      priceGrowth,
      years,
      oneTime,
    ]
  );
  const result = useMemo(() => rentalResult(input), [input]);
  const lo = useMemo(() => rentalSeries(input, -1), [input]);
  const hi = useMemo(() => rentalSeries(input, 1), [input]);

  const money = (v: number) => formatCurrencyRaw(v);
  const signed = (v: number) => fmt.money(v, currencyCode, { signed: true, decimals: 0 });
  const startYear = new Date().getFullYear();
  const yearLabel = (y: number) => String(startYear + y);
  const D = result.years.length;
  const last = result.years[D - 1];
  const yearsText = (n: number) => t('estate.years', { count: n });
  const hasResult = input.price > 0;

  const points = useMemo<BandPoint[]>(
    () => [
      { x: 0, value: result.equity, lo: result.equity, hi: result.equity, base: result.invested },
      ...result.years.map((y, i) => ({
        x: y.year,
        value: y.equity,
        lo: lo[i]?.equity ?? y.equity,
        hi: hi[i]?.equity ?? y.equity,
        base: result.invested,
      })),
    ],
    [result, lo, hi]
  );

  const milestones: BandMilestone[] = [];
  if (result.positiveCashflowYear !== null && result.positiveCashflowYear < D) {
    milestones.push({
      x: result.positiveCashflowYear,
      title: t('estate.milestones.positive'),
      lines: [
        t('estate.milestones.positiveLine', { year: yearLabel(result.positiveCashflowYear) }),
      ],
    });
  }
  if (
    result.halfLoanYear !== null &&
    result.halfLoanYear < D &&
    result.halfLoanYear !== result.positiveCashflowYear
  ) {
    milestones.push({
      x: result.halfLoanYear,
      title: t('estate.milestones.half'),
      lines: [t('estate.milestones.halfLine', { year: yearLabel(result.halfLoanYear) })],
    });
  }
  if (last) {
    milestones.push({
      x: D,
      title: t('estate.milestones.sale'),
      lines: [
        t('estate.milestones.saleLine', {
          year: yearLabel(D),
          price: formatCurrencyShort(last.value),
          loan: formatCurrencyShort(last.balance),
        }),
      ],
    });
  }

  const loadProperty = (p: RealEstate) => {
    const values = propertyDefaults(p, (v) => fmt.number(v, { maximumFractionDigits: 0 }));
    setPrice(values.price);
    if (values.rent) setRent(values.rent);
    if (values.costs) setCosts(values.costs);
  };

  const annualPct = fmt.percent(result.annualReturn, 1);
  const cashflowShare = result.invested > 0 ? (result.netMonthly * 12) / result.invested : 0;

  const saleRows: { label: string; value: string; tone?: 'gain' | 'loss' }[] = last
    ? [
        { label: t('estate.sale.finalPrice'), value: money(last.value) },
        { label: t('estate.sale.remainingLoan'), value: money(last.balance) },
        { label: t('estate.sale.repaid'), value: money(input.loan - last.balance) },
        {
          label: t('estate.sale.cumulative'),
          value: signed(last.cumulative),
          tone: last.cumulative >= 0 ? 'gain' : 'loss',
        },
        { label: t('estate.sale.equity'), value: money(last.equity) },
        {
          label: t('estate.sale.totalReturn'),
          value: `${fmt.percent(result.totalReturn, 1, { signed: true })} · ${t('estate.sale.pa', { percent: annualPct })}`,
          tone: result.totalReturn >= 0 ? 'gain' : 'loss',
        },
      ]
    : [];

  return (
    <>
      <PageHead
        eyebrow={t('estate.eyebrow')}
        title={t('estate.title')}
        description={t('estate.subtitle')}
        actions={
          properties.length > 0 ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline">
                  {t('estate.actions.loadProperty')}
                  <ChevronDown className="text-ink-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                {properties.map((p) => (
                  <DropdownMenuItem key={p.id} onSelect={() => loadProperty(p)}>
                    {p.name}
                    <span className="ml-auto pl-4 text-micro text-ink-4 num">
                      {fmt.money(parseFloat(p.purchasePrice) || 0, p.purchasePriceCurrency, {
                        decimals: 0,
                      })}
                    </span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : undefined
        }
      />

      <div className="grid grid-cols-[340px_1fr] items-start gap-[18px]">
        <div className="grid gap-[14px]">
          <Card variant="flat">
            <CardHeader className="pb-1">
              <CardTitle className="text-[16px]">{t('estate.purchase.title')}</CardTitle>
            </CardHeader>
            <div className="grid gap-[13px] px-[21px] pb-5 pt-1">
              <CalcField
                id="price"
                label={t('estate.purchase.price')}
                unit={currencyCode}
                value={price}
                onChange={setPrice}
              />
              <CalcField
                id="loan"
                label={t('estate.purchase.loan')}
                unit={currencyCode}
                value={loan}
                onChange={setLoan}
              />
              <div className="grid grid-cols-2 gap-3">
                <CalcField
                  id="loan-years"
                  label={t('estate.purchase.years')}
                  unit={t('estate.yearsUnit')}
                  value={loanYears}
                  onChange={setLoanYears}
                />
                <CalcField
                  id="rate"
                  label={t('estate.purchase.rate')}
                  unit="%"
                  value={rate}
                  onChange={setRate}
                />
              </div>
              <div>
                <ComputedRow label={t('estate.purchase.equity')} value={money(result.equity)} />
                <ComputedRow
                  label={t('estate.purchase.ltv')}
                  value={fmt.percent(result.ltv / 100, 0)}
                />
                <ComputedRow
                  label={t('estate.purchase.payment')}
                  value={money(result.monthlyPayment)}
                />
              </div>
            </div>
          </Card>
          <Card variant="flat">
            <CardHeader className="pb-1">
              <CardTitle className="text-[16px]">{t('estate.rent.title')}</CardTitle>
            </CardHeader>
            <div className="grid gap-[13px] px-[21px] pb-5 pt-1">
              <CalcField
                id="rent"
                label={t('estate.rent.rent')}
                unit={currencyCode}
                value={rent}
                onChange={setRent}
              />
              <div className="grid grid-cols-2 gap-3">
                <CalcField
                  id="costs"
                  label={t('estate.rent.costs')}
                  unit={currencyCode}
                  value={costs}
                  onChange={setCosts}
                />
                <CalcField
                  id="vacancy"
                  label={t('estate.rent.vacancy')}
                  unit={t('estate.rent.vacancyUnit')}
                  value={vacancy}
                  onChange={setVacancy}
                />
              </div>
              <p className="-mt-1 text-micro font-500 text-ink-4">{t('estate.rent.hint')}</p>
            </div>
          </Card>
          <Card variant="flat">
            <CardHeader className="pb-1">
              <CardTitle className="text-[16px]">{t('estate.projection.title')}</CardTitle>
            </CardHeader>
            <div className="grid gap-[13px] px-[21px] pb-5 pt-1">
              <div className="grid grid-cols-2 gap-3">
                <CalcField
                  id="rent-growth"
                  label={t('estate.projection.rentGrowth')}
                  unit={t('estate.projection.perYear')}
                  value={rentGrowth}
                  onChange={setRentGrowth}
                />
                <CalcField
                  id="costs-growth"
                  label={t('estate.projection.costsGrowth')}
                  unit={t('estate.projection.perYear')}
                  value={costsGrowth}
                  onChange={setCostsGrowth}
                />
              </div>
              <div className="grid grid-cols-2 gap-3">
                <CalcField
                  id="price-growth"
                  label={t('estate.projection.priceGrowth')}
                  unit={t('estate.projection.perYear')}
                  value={priceGrowth}
                  onChange={setPriceGrowth}
                />
                <CalcField
                  id="years"
                  label={t('estate.projection.years')}
                  unit={t('estate.yearsUnit')}
                  value={years}
                  onChange={setYears}
                />
              </div>
              <CalcField
                id="one-time"
                label={t('estate.projection.oneTime')}
                unit={currencyCode}
                value={oneTime}
                onChange={setOneTime}
              />
            </div>
          </Card>
          {last && (
            <Card variant="flat">
              <CardHeader className="pb-1">
                <CardTitle className="text-[16px]">
                  {t('estate.sale.title', { count: D })}
                </CardTitle>
              </CardHeader>
              <div className="px-[21px] pb-4 pt-1">
                {saleRows.map((row) => (
                  <div
                    key={row.label}
                    className="grid grid-cols-[1fr_auto] gap-x-4 border-b border-line-soft py-[9px] text-table text-ink-3 last:border-b-0"
                  >
                    <span>{row.label}</span>
                    <b
                      className={cn(
                        'font-600 text-ink num',
                        row.tone === 'gain' && 'text-gain',
                        row.tone === 'loss' && 'text-loss'
                      )}
                    >
                      {row.value}
                    </b>
                  </div>
                ))}
                <p className="mt-3 text-[10px] font-500 leading-[1.5] text-ink-4">
                  {t('estate.sale.formula')}
                </p>
              </div>
            </Card>
          )}
        </div>

        <div className="grid gap-[14px]">
          <HeroCard
            aside={
              <HeroValue
                compact
                label={t('estate.hero.cashflow')}
                value={signed(result.netMonthly)}
                tone={result.netMonthly >= 0 ? 'gain' : 'loss'}
                deltaNote={t('estate.hero.cashflowNote', { gross: signed(result.grossMonthly) })}
              />
            }
            chart={
              hasResult ? (
                <>
                  <MoonyBandChart
                    className="-mx-1 mt-4"
                    points={points}
                    height={200}
                    formatValue={money}
                    axisLabels={horizonAxisLabels(startYear, D, t('estate.chart.purchase'))}
                    milestones={milestones}
                    renderTip={(p) => ({
                      title: money(p.value),
                      lines: [
                        p.x === 0
                          ? t('estate.chart.tipStart', {
                              year: yearLabel(0),
                              invested: money(result.invested),
                            })
                          : t('estate.chart.tipRow', {
                              year: yearLabel(p.x),
                              value: formatCurrencyShort(result.years[p.x - 1]?.value ?? 0),
                              loan: formatCurrencyShort(result.years[p.x - 1]?.balance ?? 0),
                              cumulative: signed(result.years[p.x - 1]?.cumulative ?? 0),
                            }),
                      ],
                    })}
                  />
                  <ChartLegend
                    items={[
                      { label: t('estate.legend.equity'), swatch: { kind: 'line' } },
                      {
                        label: t('estate.legend.band'),
                        swatch: { kind: 'block', color: 'var(--s4)' },
                      },
                      { label: t('estate.legend.invested'), swatch: { kind: 'dash' } },
                      { label: t('estate.legend.milestone'), swatch: { kind: 'ring' } },
                    ]}
                  />
                </>
              ) : (
                <p className="grid h-[200px] place-items-center text-table text-ink-3">
                  {t('estate.empty')}
                </p>
              )
            }
          >
            <HeroValue
              label={t('estate.hero.label')}
              value={hasResult ? t('estate.hero.value', { percent: annualPct }) : '—'}
              tone={result.annualReturn >= 0 ? 'neutral' : 'loss'}
              delta={
                hasResult
                  ? t('estate.hero.total', {
                      percent: fmt.percent(result.totalReturn, 1, { signed: true }),
                      years: yearsText(D),
                    })
                  : undefined
              }
              deltaNote={
                hasResult
                  ? t('estate.hero.components', {
                      cashflow: fmt.percent(cashflowShare, 1),
                      growth: fmt.percent(input.priceGrowth / 100, 0),
                    })
                  : undefined
              }
            />
          </HeroCard>

          <Stats className="mb-0">
            <Stat
              label={t('estate.stats.equity')}
              value={money(result.invested)}
              note={t('estate.stats.equityNote', {
                equity: money(result.equity),
                once: money(input.oneTimeCosts),
              })}
            />
            <Stat
              label={t('estate.stats.grossYield')}
              value={fmt.percent(result.grossYield / 100, 1)}
              note={t('estate.stats.grossYieldNote')}
            />
            <Stat
              label={t('estate.stats.cashflow')}
              value={signed(result.netMonthly * 12)}
              tone={result.netMonthly >= 0 ? 'gain' : 'loss'}
              noteTone="neutral"
              note={t('estate.stats.cashflowNote', { percent: fmt.percent(cashflowShare, 1) })}
            />
            <Stat
              label={t('estate.stats.saleGain')}
              value={last ? money(last.value - last.balance - result.equity) : '—'}
              note={t('estate.stats.saleGainNote', { years: yearsText(D) })}
            />
          </Stats>

          <Card variant="table">
            <CardHeader>
              <div>
                <CardTitle>{t('estate.table.title')}</CardTitle>
                <CardDescription>{t('estate.table.sub')}</CardDescription>
              </div>
            </CardHeader>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-[10%]">{t('estate.table.year')}</TableHead>
                  <TableHead className="text-right">{t('estate.table.rent')}</TableHead>
                  <TableHead className="text-right">{t('estate.table.costs')}</TableHead>
                  <TableHead className="text-right">{t('estate.table.payments')}</TableHead>
                  <TableHead className="text-right">{t('estate.table.cashflow')}</TableHead>
                  <TableHead className="text-right">{t('estate.table.balance')}</TableHead>
                  <TableHead className="text-right">{t('estate.table.value')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {result.years.map((y) => (
                  <TableRow key={y.year} className="h-[42px]">
                    <TableCell className="font-650 text-ink">{yearLabel(y.year)}</TableCell>
                    <TableCell className="text-right num">{money(y.rent)}</TableCell>
                    <TableCell className="text-right num">{money(y.costs)}</TableCell>
                    <TableCell className="text-right num">{money(y.payments)}</TableCell>
                    <TableCell className="text-right num">
                      <b className={cn('font-650', y.cashflow >= 0 ? 'text-gain' : 'text-loss')}>
                        {signed(y.cashflow)}
                      </b>
                    </TableCell>
                    <TableCell className="text-right num">{money(y.balance)}</TableCell>
                    <TableCell className="text-right font-650 text-ink num">
                      {money(y.value)}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
              <span className="num">
                {last
                  ? t('estate.table.footer', {
                      years: yearsText(D),
                      cumulative: signed(last.cumulative),
                    })
                  : ''}
              </span>
              <span>{t('estate.table.vacancyNote')}</span>
            </div>
          </Card>
        </div>
      </div>
    </>
  );
}
