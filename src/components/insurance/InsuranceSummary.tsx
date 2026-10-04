import { useTranslation } from 'react-i18next';
import type { InsuranceMetrics } from '@/utils/insurance';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useDurationText } from '@/hooks/use-duration-text';
import { Stat, Stats } from '@/components/common/Stat';
import type { CurrencyCode } from '@shared/currencies';

interface InsuranceSummaryProps {
  metrics: InsuranceMetrics;
  today: number;
}

/**
 * Four stats (prototype insurance.html): yearly premium with its monthly
 * equivalent, the next payment and what falls due with it, total coverage by
 * type, and the nearest anniversary or contract end.
 */
export function InsuranceSummary({ metrics, today }: InsuranceSummaryProps) {
  const { t } = useTranslation('insurance');
  const { formatCurrency, convert, currencyCode } = useCurrency();
  const fmt = useFormat();
  const duration = useDurationText('insurance');
  const compact = (czk: number) =>
    fmt.money(convert(czk, 'CZK', currencyCode as CurrencyCode), currencyCode, {
      compact: true,
      decimals: 2,
    });

  const next = metrics.nextPayment;
  const date = metrics.nextDate;
  const breakdown = metrics.coverageByType
    .slice(0, 3)
    .map((e) => `${t(`typeShort.${e.type}`, { defaultValue: e.type })} ${compact(e.czk)}`)
    .join(', ');

  return (
    <Stats>
      <Stat
        label={t('summary.yearly')}
        value={formatCurrency(metrics.yearlyTotal)}
        note={t('summary.yearlyNote', {
          monthly: formatCurrency(metrics.yearlyTotal / 12),
          policies: t('policies', { count: metrics.activeCount }),
        })}
      />
      <Stat
        label={t('summary.nextPayment')}
        value={next ? fmt.day(next.day) : '—'}
        note={
          next
            ? t('summary.nextPaymentNote', {
                amount: formatCurrency(next.amountCzk),
                policies:
                  next.rows.length === 1
                    ? next.rows[0].policy.policyName
                    : t('payments', { count: next.rows.length }),
              })
            : t('summary.noPayments')
        }
      />
      <Stat
        label={t('summary.coverage')}
        value={metrics.coverageTotal > 0 ? compact(metrics.coverageTotal) : '—'}
        note={
          metrics.coverageTotal > 0
            ? t('summary.coverageNote', { breakdown })
            : t('summary.noCoverage')
        }
      />
      <Stat
        label={t('summary.anniversary')}
        value={date ? fmt.day(date.day) : '—'}
        note={
          date
            ? t(date.kind === 'end' ? 'summary.endNote' : 'summary.anniversaryNote', {
                name: date.row.policy.policyName,
                duration: duration.ahead(today, date.day),
              })
            : t('summary.noAnniversary')
        }
      />
    </Stats>
  );
}
