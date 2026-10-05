import { useMutation } from '@tanstack/react-query';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import type { Milestone, MilestoneKind, MilestoneState } from '@shared/schema';
import { queryClient } from '@/lib/queryClient';
import { milestonesApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useMilestoneMutedKinds } from '@/hooks/use-milestones';
import { isUpkeep, kindsOf, type MilestoneGroupId } from '@/utils/milestones';

/**
 * "Skrýt", "Připomenout za týden / měsíc" and "Nepřipomínat …", each with a "Vrátit" toast.
 * Every successful mutation refreshes ["milestones"] through the query client.
 */
export function useMilestoneActions() {
  const { t } = useTranslation('milestones');
  const { t: tc } = useTranslation('common');
  const { data: muted = [] } = useMilestoneMutedKinds();

  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  const undo = useMutation({
    mutationFn: (key: string) => milestonesApi.clearState(key),
    onError,
  });

  const setState = useMutation({
    mutationFn: ({ m, state }: { m: Milestone; state: MilestoneState }) =>
      milestonesApi.setState(m.key, state),
    onSuccess: (_data, { m, state }) => {
      const message =
        state === 'done'
          ? t('toast.hidden')
          : t(isUpkeep(m) ? 'toast.snoozedMonth' : 'toast.snoozedWeek');
      toast(message, { action: { label: t('actions.undo'), onClick: () => undo.mutate(m.key) } });
    },
    onError,
  });

  const setMuted = useMutation({
    mutationFn: (kinds: MilestoneKind[]) => milestonesApi.setMutedKinds(kinds),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['milestone-muted-kinds'] }),
    onError,
  });

  const withGroup = (group: MilestoneGroupId, on: boolean): MilestoneKind[] => {
    const kinds = kindsOf(group);
    return on
      ? Array.from(new Set([...muted, ...kinds]))
      : muted.filter((kind) => !kinds.includes(kind));
  };

  return {
    hide: (m: Milestone) => setState.mutate({ m, state: 'done' }),
    remindLater: (m: Milestone) => setState.mutate({ m, state: 'snoozed' }),
    muteGroup: (group: MilestoneGroupId) => {
      const previous = muted;
      setMuted.mutate(withGroup(group, true), {
        onSuccess: () =>
          toast(t('toast.muted', { group: t(`groups.${group}.label`) }), {
            action: { label: t('actions.undo'), onClick: () => setMuted.mutate(previous) },
          }),
      });
    },
    setGroupMuted: (group: MilestoneGroupId, mutedOn: boolean) =>
      setMuted.mutate(withGroup(group, mutedOn)),
  };
}
