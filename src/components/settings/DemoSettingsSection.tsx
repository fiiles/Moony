/**
 * Settings in the demo profile (ADR 0037, prototype scene 4): the section "Ukázka" with the three
 * things to do with the sample data, and the panel that replaces a section the demo locks.
 */
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Lock } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/common/EmptyState';
import { ResetDemoDialog } from '@/components/demo/ResetDemoDialog';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import type { SettingsSectionId } from '@/components/settings/settings-sections';
import { useAuth } from '@/hooks/use-auth';
import { useFormat } from '@/lib/use-format';
import { dayRelation } from '@/utils/demo-profile';

/** /settings/demo */
export function DemoSettingsSection() {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { storedDemo, closeDemoMutation } = useAuth();
  const [resetOpen, setResetOpen] = useState(false);
  const leaving = closeDemoMutation.isPending;

  const at = storedDemo?.lastOpenedAt ?? null;
  const sub =
    at === null
      ? t('demo.sub')
      : {
          today: () => t('demo.subToday', { time: fmt.time(at) }),
          yesterday: () => t('demo.subYesterday', { time: fmt.time(at) }),
          earlier: () => t('demo.subOn', { date: fmt.dateTime(at) }),
        }[dayRelation(at, new Date())]();

  return (
    <>
      <SettingsCard title={t('demo.title')} description={sub}>
        <SettingsRow label={tc('demo.createProfile')} hint={t('demo.createHint')}>
          <Button disabled={leaving} onClick={() => closeDemoMutation.mutate('ownProfile')}>
            {tc('demo.createProfile')}
          </Button>
        </SettingsRow>
        <SettingsRow label={tc('demo.reset.title')} hint={t('demo.resetHint')}>
          <Button variant="outline" onClick={() => setResetOpen(true)}>
            {tc('demo.reset.title')}
          </Button>
        </SettingsRow>
        <SettingsRow label={tc('demo.exit')} hint={t('demo.exitHint')}>
          <Button
            variant="outline"
            disabled={leaving}
            onClick={() => closeDemoMutation.mutate('choice')}
          >
            {tc('demo.exit')}
          </Button>
        </SettingsRow>
      </SettingsCard>
      <ResetDemoDialog open={resetOpen} onOpenChange={setResetOpen} />
    </>
  );
}

/** A section the demo lists but locks: what it is, why, and the way to an own profile. */
export function LockedInDemoSection({ id }: { id: SettingsSectionId }) {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const { closeDemoMutation } = useAuth();
  return (
    <EmptyState
      icon={<Lock />}
      title={t('demo.locked.title', { section: t(`sections.${id}`) })}
      description={t(`demo.locked.${id}`)}
      action={
        <Button
          variant="outline"
          disabled={closeDemoMutation.isPending}
          onClick={() => closeDemoMutation.mutate('ownProfile')}
        >
          {tc('demo.createProfile')}
        </Button>
      }
    />
  );
}
