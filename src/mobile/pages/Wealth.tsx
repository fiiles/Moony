import { useTranslation } from 'react-i18next';
import { Screen } from '@/mobile/shell/Screen';

/** Majetek tab root: what you own (spec §8.8). Placeholder until Phase 4. */
export default function Wealth() {
  const { t } = useTranslation('mobile');
  return (
    <Screen title={t('tabs.wealth')} large>
      <p className="mx-0.5 mt-1 text-ink-3">{t('shell.placeholder')}</p>
    </Screen>
  );
}
