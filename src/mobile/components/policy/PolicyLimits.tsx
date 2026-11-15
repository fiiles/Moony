import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { recordCurrency } from '@shared/currencies';
import { useCurrency } from '@/lib/currency';
import { limitsByValue, type InsuranceRow } from '@/utils/insurance';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { useMoneyText } from '@/mobile/hooks/use-money-text';

/** Whole amounts: a limit is a round figure of the contract. */
const WHOLE = { decimals: 0 } as const;

/**
 * "Limity krytí" (spec §8.8 "Pojistka", prototype frame 14): a row per limit — its title and its
 * amount in its own currency — from the largest to the smallest by value in the main currency, as
 * the desktop ranks them, the count in the head. The limits are not summed: a property limit and a
 * health limit cover different things, and their total says nothing (audit D16). No limits are one
 * line that says how to add them.
 */
export function PolicyLimits({ row }: { row: InsuranceRow }) {
  const { t: ti } = useTranslation('insurance');
  const moneyText = useMoneyText();
  const { convert } = useCurrency();
  const limits = useMemo(
    () => limitsByValue(row.policy.limits, (amount, from) => convert(amount, from)),
    [row.policy.limits, convert]
  );

  return (
    <section>
      <SectionHead
        title={ti('detail.limits.title')}
        note={limits.length > 0 ? String(limits.length) : undefined}
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
