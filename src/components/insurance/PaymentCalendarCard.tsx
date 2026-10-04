import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { CurrencyCode } from '@shared/currencies';
import { paymentCalendar, type InsuranceRow } from '@/utils/insurance';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { Badge } from '@/components/ui/badge';
import { MoonyBarChart } from '@/components/charts/MoonyBarChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { TrendCard, TREND_CHART_HEIGHT } from '@/components/charts/TrendCard';

interface PaymentCalendarCardProps {
  rows: InsuranceRow[];
  today: number;
}

/**
 * Twelve-month payment calendar (prototype insurance.html): monthly premiums
 * stacked with the quarterly, semi-annual and annual payments, anniversaries
 * marked, and the month to prepare a reserve for in the legend note.
 */
export function PaymentCalendarCard({ rows, today }: PaymentCalendarCardProps) {
  const { t } = useTranslation('insurance');
  const { formatCurrency, convert, currencyCode } = useCurrency();
  const fmt = useFormat();

  const months = useMemo(() => paymentCalendar(rows, today), [rows, today]);
  const toDisplay = (czk: number) => convert(czk, 'CZK', currencyCode as CurrencyCode);
  const total = months.reduce((s, m) => s + m.monthly + m.other, 0);
  if (total <= 0) return null;

  const totals = months.map((m) => m.monthly + m.other);
  const peakIndex = totals.indexOf(Math.max(...totals));
  const peak = months[peakIndex];
  const even = Math.max(...totals) <= Math.min(...totals) * 1.3;
  // "Lis", "Pro", "Led 27": the short month alone (the year only where it changes)
  const monthLabel = (sec: number) => {
    const d = new Date(sec * 1000);
    const name = fmt.month(d, { month: 'short', year: undefined });
    return d.getUTCMonth() === 0 ? `${name} ${String(d.getUTCFullYear()).slice(2)}` : name;
  };

  return (
    <TrendCard
      title={t('calendar.title')}
      subtitle={t('calendar.subtitle')}
      aside={
        <Badge variant="outline">{t('calendar.total', { amount: formatCurrency(total) })}</Badge>
      }
      legend={
        <ChartLegend
          items={[
            { label: t('calendar.monthly'), swatch: { kind: 'block', color: 'var(--s1)' } },
            { label: t('calendar.other'), swatch: { kind: 'block', color: 'var(--s3)' } },
          ]}
          note={
            even
              ? t('calendar.evenNote', { amount: formatCurrency(total / 12) })
              : t('calendar.peakNote', {
                  month: fmt.month(new Date(peak.month * 1000)),
                  amount: formatCurrency(peak.monthly + peak.other),
                })
          }
        />
      }
    >
      <MoonyBarChart
        bars={months.map((m) => ({
          label: monthLabel(m.month),
          a: toDisplay(m.monthly),
          b: toDisplay(m.other),
        }))}
        stacked
        height={TREND_CHART_HEIGHT}
        names={[t('calendar.monthly'), t('calendar.other')]}
        formatValue={(v) => fmt.money(v, currencyCode, { decimals: 0 })}
        marks={months.flatMap((m, index) =>
          m.anniversaries.map((r) => ({
            index,
            title: t('calendar.anniversary', { name: r.policy.policyName }),
            lines: [
              `${fmt.day(r.anniversary!)} · ${formatCurrency(r.yearlyCzk)} ${t('table.perYearShort')}`,
            ],
          }))
        )}
      />
    </TrendCard>
  );
}
