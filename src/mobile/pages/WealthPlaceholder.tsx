import { useTranslation } from 'react-i18next';
import { useBack } from '@/mobile/hooks/use-back';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/**
 * What the Nemovitosti list route shows. Nothing in the app links to it; the route stays so a deep
 * link to `/wealth/real-estate` lands on a screen that names the class and says its page is
 * coming, never on a blank.
 */
export default function RealEstatePlaceholder() {
  const { t } = useTranslation('mobile');
  const { t: td } = useTranslation('dashboard');
  const back = useBack(routes.wealth);

  return (
    <Screen
      title={td('cards.realEstate')}
      leading={<NavTextButton label={t('tabs.wealth')} onClick={back} chevron />}
    >
      <p className="mx-0.5 mt-2 text-m-body text-ink-3">{t('wealth.placeholder.classPage')}</p>
    </Screen>
  );
}
