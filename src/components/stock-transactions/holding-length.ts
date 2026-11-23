import type { TFunction } from 'i18next';

/** The holding-test length as a phrase: whole years from 12 months up, otherwise months. */
export function holdingLength(t: TFunction<'stockTransactions'>, months: number): string {
  return months % 12 === 0
    ? t('length.years', { count: months / 12 })
    : t('length.months', { count: months });
}
