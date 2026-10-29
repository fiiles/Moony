import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { recordCurrency } from '@shared/currencies';
import { useCurrency } from '@/lib/currency';
import { limitsByValue, type InsuranceRow } from '@/utils/insurance';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { statMoneyOptions } from '@/mobile/lib/stat-money';

/** Whole amounts: a limit is a round figure of the contract. */
const WHOLE = { decimals: 0 } as const;

/**
 * "Limity krytí" (spec §8.8 "Pojistka", prototype frame 14): the head with the sum of the limits
 * in the main currency ("celkem 6,65 mil. Kč"), then a row per limit — its title and its amount in
 * its own currency — from the largest to the smallest by value in the main currency, as the
 * desktop ranks them. The sum is left out when a limit has no rate (the desktop's "—" would only
 * sit in a head that has room for one number) or there are no limits, which are one line that says
 * how to add them.
 */
export function PolicyLimits({ row }: { row: InsuranceRow }) {
  const { t } = useTranslation('mobile');
  const { t: ti } = useTranslation('insurance');
  const moneyText = useMoneyText();
  const { currencyCode, convert } = useCurrency();
  const limits = useMemo(
    () => limitsByValue(row.policy.limits, (amount, from) => convert(amount, from)),
    [row.policy.limits, convert]
  );

  const total =
    row.coverage !== null && row.coverageKnown > 0
      ? moneyText(row.coverage, currencyCode, statMoneyOptions(row.coverage))
      : null;

  return (
    <section>
      <SectionHead
        title={ti('detail.limits.title')}
        note={total === null ? undefined : t('policy.limitsTotal', { amount: total })}
      />
      {limits.length === 0 ? (
        <p className="mx-0.5 mb-3.5 text-m-sub text-ink-3">{ti('detail.limits.empty')}</p>
      ) : (
        <EntityRows>
          {limits.map((limit, index) => (
            <EntityRow
              key={`${limit.title}-${index}`}
              title={limit.title}
              value={moneyText(Number(limit.amount) || 0, recordCurrency(limit.currency), WHOLE)}
            />
          ))}
        </EntityRows>
      )}
    </section>
  );
}
