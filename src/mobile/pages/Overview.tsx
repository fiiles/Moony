import { useTranslation } from 'react-i18next';
import { Screen } from '@/mobile/shell/Screen';

/** Přehled tab root (spec §8.3). Placeholder until Task 1.5. */
export default function Overview() {
  const { t } = useTranslation('mobile');
  return (
    <Screen title={t('tabs.overview')} large>
      <p className="mx-0.5 mt-1 text-ink-3">{t('shell.placeholder')}</p>
    </Screen>
  );
}
