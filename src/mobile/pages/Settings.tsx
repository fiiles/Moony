import { useTranslation } from 'react-i18next';
import { useBack } from '@/mobile/hooks/use-back';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/** Nastavení from the avatar on Přehled (spec §8.9). Placeholder until Phase 5. */
export default function Settings() {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const back = useBack(routes.overview);
  return (
    <Screen
      title={tc('nav.settings')}
      tabBar={false}
      leading={<NavTextButton label={t('tabs.overview')} onClick={back} chevron />}
    >
      <p className="mx-0.5 mt-2 text-m-body text-ink-3">{t('shell.placeholder')}</p>
    </Screen>
  );
}
