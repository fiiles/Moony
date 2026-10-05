import { useTranslation } from 'react-i18next';
import { Switch } from '@/components/ui/switch';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { useMilestoneMutedKinds } from '@/hooks/use-milestones';
import { useMilestoneActions } from '@/hooks/use-milestone-mutations';
import { MILESTONE_GROUP_IDS, kindsOf } from '@/utils/milestones';

/** Settings → Obecné → Připomínky na přehledu: one switch per reminder group. */
export function MilestoneSettingsCard() {
  const { t } = useTranslation('milestones');
  const { data: muted = [] } = useMilestoneMutedKinds();
  const { setGroupMuted } = useMilestoneActions();

  return (
    <SettingsCard title={t('settings.title')} description={t('settings.description')}>
      {MILESTONE_GROUP_IDS.map((group) => {
        const on = !kindsOf(group).every((kind) => muted.includes(kind));
        return (
          <SettingsRow
            key={group}
            htmlFor={`milestone-group-${group}`}
            label={t(`groups.${group}.label`)}
            hint={t(`groups.${group}.hint`)}
          >
            <Switch
              id={`milestone-group-${group}`}
              checked={on}
              onCheckedChange={(checked) => setGroupMuted(group, !checked)}
            />
          </SettingsRow>
        );
      })}
    </SettingsCard>
  );
}
