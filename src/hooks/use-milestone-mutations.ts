import { useMutation } from '@tanstack/react-query';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import type { Milestone, MilestoneState } from '@shared/schema';
import { queryClient } from '@/lib/queryClient';
import { milestonesApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';

/** "Vyřízeno" and "Odložit o týden", each with a "Vrátit" toast. */
export function useMilestoneActions() {
  const { t } = useTranslation('milestones');
  const { t: tc } = useTranslation('common');

  // Milestone state never affects net worth — the milestones key only.
  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['milestones'] });
  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  const undo = useMutation({
    mutationFn: (key: string) => milestonesApi.clearState(key),
    onSuccess: invalidate,
    onError,
  });

  const setState = useMutation({
    mutationFn: ({ key, state }: { key: string; state: MilestoneState }) =>
      milestonesApi.setState(key, state),
    onSuccess: (_data, { key, state }) => {
      invalidate();
      toast(t(state === 'done' ? 'toast.done' : 'toast.snoozed'), {
        action: { label: t('actions.undo'), onClick: () => undo.mutate(key) },
      });
    },
    onError,
  });

  return {
    markDone: (m: Milestone) => setState.mutate({ key: m.key, state: 'done' }),
    snooze: (m: Milestone) => setState.mutate({ key: m.key, state: 'snoozed' }),
  };
}
