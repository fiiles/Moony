import { Lock } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { useAuth } from '@/hooks/use-auth';
import { useBack } from '@/mobile/hooks/use-back';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/**
 * Nastavení from the avatar on Přehled (spec §8.9). Placeholder until Phase 5, which brings the
 * real Settings and its Zabezpečení section; until then the lock button lives here so the app can
 * be locked by hand.
 */
export default function Settings() {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const { lockMutation } = useAuth();
  const back = useBack(routes.overview);
  return (
    <Screen
      title={tc('nav.settings')}
      tabBar={false}
      leading={<NavTextButton label={t('tabs.overview')} onClick={back} chevron />}
    >
      <p className="mx-0.5 mt-2 text-m-body text-ink-3">{t('shell.placeholder')}</p>
      <Button
        variant="outline"
        className="mt-4 h-12 w-full rounded-[10px] text-m-row"
        onClick={() => lockMutation.mutate()}
        disabled={lockMutation.isPending}
      >
        <Lock aria-hidden />
        {tc('nav.lockApp')}
      </Button>
    </Screen>
  );
}
