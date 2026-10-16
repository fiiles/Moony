import { useTranslation } from 'react-i18next';
import { Lock } from 'lucide-react';
import { useAuth } from '@/hooks/use-auth';
import { Button } from '@/components/ui/button';
import { Screen } from '@/mobile/shell/Screen';

/** Přehled tab root (spec §8.3). Placeholder until Task 1.5. */
export default function Overview() {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const { lockMutation } = useAuth();
  return (
    <Screen title={t('tabs.overview')} large>
      <p className="mx-0.5 mt-1 text-ink-3">{t('shell.placeholder')}</p>
      {/* TEMPORARY (Task 1.4, removed in Task 1.6 with the real Settings → Zabezpečení): locks the app. */}
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
