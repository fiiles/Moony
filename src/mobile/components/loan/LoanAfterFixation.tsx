import { useTranslation } from 'react-i18next';
import { useLoanText } from '@/hooks/use-loan-text';
import { useFormat } from '@/lib/use-format';
import type { LoanRow } from '@/utils/loans';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { fixationScenarios, type LoanDetailStats } from '@/mobile/lib/loan-detail';
import { WHOLE } from '@/mobile/lib/money-rule';

/**
 * "Po konci fixace" (audit D13): what the monthly payment would be if the loan were refixed at
 * today's rate or one point either side of it, a row each, with the month the fixation ends in
 * the head. Shown only while a fixation is still ahead; the "Konec fixace" stat keeps one short
 * note.
 */
export function LoanAfterFixation({ row, stats }: { row: LoanRow; stats: LoanDetailStats }) {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const text = useLoanText();
  if (stats.fixationEnd === null || stats.balance <= 0) return null;

  const rate = Number(row.loan.interestRate) || 0;
  const scenarios = fixationScenarios(stats.balance, rate, stats.remaining);
  return (
    <section>
      <SectionHead title={t('loan.afterFixation.title')} note={text.monthYear(stats.fixationEnd)} />
      <EntityRows>
        {scenarios.map((scenario) => (
          <EntityRow
            key={scenario.rate}
            title={t('loan.afterFixation.rate', { rate: fmt.percent(scenario.rate / 100, 2) })}
            sub={
              scenario.delta === 0
                ? t('loan.afterFixation.current')
                : t('loan.afterFixation.delta', {
                    // The typographic minus, as every signed figure of the app writes it
                    delta: `${scenario.delta > 0 ? '+' : '−'}${fmt.number(
                      Math.abs(scenario.delta),
                      {
                        maximumFractionDigits: 2,
                      }
                    )}`,
                  })
            }
            value={moneyText(scenario.payment, row.currency, WHOLE)}
            valueSub={t('loan.afterFixation.perMonth')}
          />
        ))}
      </EntityRows>
    </section>
  );
}
