import type { TFunction } from 'i18next';
import type { Formatters } from '@/lib/format';
import type { PlansCardSummary } from '@/mobile/lib/plan-rows';

/**
 * "Utraceno S z P na rok N": the whole year's spend and plan (closed plans included, spec M17),
 * in the report currency. A total that could not be converted reads "—", never 0.
 */
export function yearSpentLine(
  t: TFunction<'mobile'>,
  money: Formatters['money'],
  summary: PlansCardSummary
): string {
  const text = (value: number | null) =>
    value === null ? '—' : money(value, summary.currency, { decimals: 0 });
  return t('plans.spent', {
    spent: text(summary.spent),
    planned: text(summary.planned),
    year: summary.year,
  });
}
