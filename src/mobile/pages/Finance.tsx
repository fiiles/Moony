import { useTranslation } from 'react-i18next';
import { Screen } from '@/mobile/shell/Screen';

/** Finance tab root: accounts, loans, policies (spec §8.4). Placeholder until Phase 2. */
export default function Finance() {
  const { t } = useTranslation('mobile');
  return (
    <Screen title={t('tabs.finance')} large>
      <p className="mx-0.5 mt-1 text-ink-3">{t('shell.placeholder')}</p>
    </Screen>
  );
}
