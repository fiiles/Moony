import { useMutation } from '@tanstack/react-query';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import type { Milestone, MilestoneKind, MilestoneState } from '@shared/schema';
import { queryClient } from '@/lib/queryClient';
import { milestonesApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useMilestoneMutedKinds } from '@/hooks/use-milestones';
import { isUpkeep, kindsOf, type MilestoneGroupId } from '@/utils/milestones';

/** `mutedGroup` is set only for "Nepřipomínat …", the one change that gets a toast. */
interface MutedVariables {
  kinds: MilestoneKind[];
  mutedGroup?: MilestoneGroupId;
}

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

  // The mute toast lives in the mutation options, not in a per-call `mutate(..., { onSuccess })`:
  // a row calling the hook can unmount when its item disappears, which drops call-level callbacks.
  const setMuted = useMutation({
    mutationFn: ({ kinds }: MutedVariables) => milestonesApi.setMutedKinds(kinds),
    onSuccess: (_data, { mutedGroup }) => {
      void queryClient.invalidateQueries({ queryKey: ['milestone-muted-kinds'] });
      if (mutedGroup) {
        toast(t('toast.muted', { group: t(`groups.${mutedGroup}.label`) }), {
          action: { label: t('actions.undo'), onClick: () => setGroupMuted(mutedGroup, false) },
        });
      }
    },
    onError,
  });

  // Built from the cache, not from the render closure: two quick changes (or an undo after
  // another switch) must not overwrite each other with a stale list.
  const withGroup = (group: MilestoneGroupId, on: boolean): MilestoneKind[] => {
    const current = queryClient.getQueryData<MilestoneKind[]>(['milestone-muted-kinds']) ?? muted;
    const kinds = kindsOf(group);
    return on
      ? Array.from(new Set([...current, ...kinds]))
      : current.filter((kind) => !kinds.includes(kind));
  };

  function setGroupMuted(group: MilestoneGroupId, mutedOn: boolean): void {
    setMuted.mutate({ kinds: withGroup(group, mutedOn) });
  }

  return {
    hide: (m: Milestone) => setState.mutate({ m, state: 'done' }),
    remindLater: (m: Milestone) => setState.mutate({ m, state: 'snoozed' }),
    muteGroup: (group: MilestoneGroupId) =>
      setMuted.mutate({ kinds: withGroup(group, true), mutedGroup: group }),
    setGroupMuted,
  };
}
