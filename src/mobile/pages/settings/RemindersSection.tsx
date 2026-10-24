import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Input } from '@/components/ui/input';
import { useAuth } from '@/hooks/use-auth';
import { useMilestoneActions } from '@/hooks/use-milestone-mutations';
import { useMilestoneMutedKinds } from '@/hooks/use-milestones';
import { useSettingsMutations } from '@/hooks/use-settings-mutations';
import { parseNoticeDays } from '@/utils/insurance-notice';
import { MILESTONE_GROUP_IDS, kindsOf } from '@/utils/milestones';
import { SectionPage } from '@/mobile/components/SectionPage';
import { SwitchRow } from '@/mobile/components/SwitchRow';

/** The card the section's controls sit in (`.m-card`), rows `line-soft` apart. */
const CARD =
  'mb-3.5 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-3 shadow-e1';
/** The same card around one field, which is no list of rows. */
const FIELD_CARD = 'mb-3.5 rounded-r4 border border-line bg-paper px-4 py-3 shadow-e1';

/**
 * Nastavení → Připomínky (spec §8.9): the desktop's "Overview reminders" — one switch per group of
 * reminders, then the insurance notice in days. The switches are the milestone mute list, which
 * the desktop shares: a group turned off here is off on Přehled, in the agenda and in the
 * notifications, and on the computer too. Wording from `milestones`, as on the desktop card.
 */
export default function RemindersSection() {
  const { t } = useTranslation('mobile');
  const { t: tm } = useTranslation('milestones');
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
    <SectionPage title={t('settings.rows.reminders')} lead={tm('settings.description')}>
      <div className={CARD}>
        {MILESTONE_GROUP_IDS.map((group) => (
          <SwitchRow
            key={group}
            id={`milestone-group-${group}`}
            label={tm(`groups.${group}.label`)}
            sub={tm(`groups.${group}.hint`)}
            checked={!kindsOf(group).every((kind) => muted?.includes(kind))}
            // Until the muted list has loaded the switches would flash ON and then flip.
            disabled={muted === undefined}
            onCheckedChange={(on) => setGroupMuted(group, !on)}
          />
        ))}
      </div>

      <div className={FIELD_CARD}>
        <label htmlFor="insurance-notice-days" className="block text-m-body text-ink">
          {tm('settings.insuranceNotice')}
        </label>
        <p id="insurance-notice-hint" className="m-0 mb-2.5 mt-0.5 text-m-sub text-ink-4">
          {tm('settings.insuranceNoticeHint')}
        </p>
        <Input
          id="insurance-notice-days"
          aria-describedby="insurance-notice-hint"
          className="h-12 rounded-[10px] text-m-field"
          // Plain text, not type="number": WebKit reports '' for a partial entry such as "-" or
          // "1e", which would read as "no reminder"; the parser must see what was typed.
          type="text"
          inputMode="numeric"
          enterKeyHint="done"
          value={shown}
          disabled={!user || updateProfile.isPending}
          onChange={(e) => setTyped(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') commit();
          }}
        />
      </div>
    </SectionPage>
  );
}
