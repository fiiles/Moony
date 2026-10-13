import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { useAuth } from '@/hooks/use-auth';
import { useMilestoneMutedKinds } from '@/hooks/use-milestones';
import { useMilestoneActions } from '@/hooks/use-milestone-mutations';
import { useSettingsMutations } from '@/hooks/use-settings-mutations';
import { parseNoticeDays } from '@/utils/insurance-notice';
import { MILESTONE_GROUP_IDS, kindsOf } from '@/utils/milestones';

/** Settings → General → Overview reminders: one switch per reminder group, then the insurance notice. */
export function MilestoneSettingsCard() {
  const { t } = useTranslation('milestones');
  const { t: tc } = useTranslation('common');
  const { user } = useAuth();
  const { updateProfile } = useSettingsMutations();
  const { data: muted } = useMilestoneMutedKinds();
  const { setGroupMuted } = useMilestoneActions();

  // The notice the milestones use (0 = no reminder); the field shows what is typed, else this.
  const stored = user?.insuranceNoticeDays ?? 0;
  const [typed, setTyped] = useState<string | null>(null);
  const shown = typed ?? String(stored);
  const commit = () => {
    if (updateProfile.isPending) return;
    const days = parseNoticeDays(shown);
    if (days === null) {
      setTyped(null);
      toast.error(tc('validation.insuranceNoticeInvalid'));
      return;
    }
    if (days === stored) {
      setTyped(null);
      return;
    }
    // Keep the typed text until the save settles so the field does not flash the old value.
    updateProfile.mutate({ insuranceNoticeDays: days }, { onSettled: () => setTyped(null) });
  };

  return (
    <SettingsCard title={t('settings.title')} description={t('settings.description')}>
      {MILESTONE_GROUP_IDS.map((group) => {
        const on = !kindsOf(group).every((kind) => muted?.includes(kind));
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
              // Until the muted list has loaded the switches would flash ON and then flip.
              disabled={muted === undefined}
              onCheckedChange={(checked) => setGroupMuted(group, !checked)}
            />
          </SettingsRow>
        );
      })}
      <SettingsRow
        htmlFor="insurance-notice-days"
        label={t('settings.insuranceNotice')}
        hint={t('settings.insuranceNoticeHint')}
      >
        <Input
          id="insurance-notice-days"
          className="w-24"
          // Plain text, not type="number": WebKit reports '' for a partial entry such as "-" or
          // "1e", which would read as "no reminder"; the parser must see what was typed.
          type="text"
          inputMode="numeric"
          value={shown}
          disabled={!user || updateProfile.isPending}
          onChange={(e) => setTyped(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') commit();
          }}
        />
      </SettingsRow>
    </SettingsCard>
  );
}
