import { useTranslation } from 'react-i18next';
import { Screen } from '@/mobile/shell/Screen';

/** Cashflow tab root, with budgets (spec §8.7). Placeholder until Phase 3. */
export default function Cashflow() {
  const { t } = useTranslation('mobile');
  return (
    <Screen title={t('tabs.cashflow')} large>
      <p className="mx-0.5 mt-1 text-ink-3">{t('shell.placeholder')}</p>
    </Screen>
  );
}
