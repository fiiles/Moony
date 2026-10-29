import { useMutation, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { spendingPlansApi } from '@/lib/tauri-api';
import { ApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import type { InsertSpendingPlan, SpendingPlan, SpendingPlanAssignment } from '@shared/schema';

/**
 * Keys a plan write can change. A plan and a payment's tag are labels: they change no money or
 * asset state (no balance, no category, no net worth), so the money-mutation contract of
 * `docs/standards/typescript-frontend.md` rule 3 (`portfolio-metrics`, `cashflow-report`,
 * `recordSnapshot()`, `portfolio-history`) does not apply here.
 */
const SPENDING_PLAN_KEYS = [
  'spending-plans',
  'spending-plan',
  'spending-plan-transactions',
  'spending-plan-candidates',
  'bank-transactions',
] as const;

function refreshSpendingPlans(client: QueryClient): Promise<void> {
  return Promise.all(
    SPENDING_PLAN_KEYS.map((key) => client.invalidateQueries({ queryKey: [key] }))
  ).then(() => undefined);
}

/** The plan form shows a taken name under the name field, so its toast would only repeat it. */
function isNameTaken(error: Error): boolean {
  return error instanceof ApiError && error.key === 'validation.spendingPlanNameTaken';
}

export function useSpendingPlanMutations() {
  const client = useQueryClient();
  const { t } = useTranslation('spending_plans');
  const { t: tc } = useTranslation('common');
  const refresh = () => refreshSpendingPlans(client);
  const showError = (error: Error) => toast.error(translateApiError(error, tc));

  /** The caller toasts (a plan created from a picker is announced with its assignment). */
  const createPlan = useMutation({
    mutationFn: (data: InsertSpendingPlan) => spendingPlansApi.create(data),
    onSuccess: refresh,
    onError: (error: Error) => {
      if (!isNameTaken(error)) showError(error);
    },
  });

  /** Take-over: all or nothing; the caller toasts. */
  const createPlans = useMutation({
    mutationFn: (items: InsertSpendingPlan[]) => spendingPlansApi.createMany(items),
    onSuccess: refresh,
    onError: showError,
  });

  const updatePlan = useMutation({
    mutationFn: ({ id, data }: { id: string; data: InsertSpendingPlan }) =>
      spendingPlansApi.update(id, data),
    onSuccess: async () => {
      await refresh();
      toast.success(t('toast.saved'));
    },
    onError: (error: Error) => {
      if (!isNameTaken(error)) showError(error);
    },
  });

  /** Close or reopen; reversible, so no confirmation, and the toast offers Undo. */
  const setClosed = useMutation({
    mutationFn: ({ id, closed }: { id: string; closed: boolean }) =>
      spendingPlansApi.setClosed(id, closed),
    onSuccess: async (plan: SpendingPlan, { closed }) => {
      await refresh();
      const undo = () =>
        spendingPlansApi.setClosed(plan.id, !closed).then(refresh, (error: Error) => {
          showError(error);
        });
      toast.success(t(closed ? 'toast.closed' : 'toast.reopened', { name: plan.name }), {
        duration: 7000,
        action: { label: t('actions.undo'), onClick: () => void undo() },
      });
    },
    onError: showError,
  });

  /** Deleting keeps the payments; they only lose the tag. */
  const deletePlan = useMutation({
    mutationFn: (plan: Pick<SpendingPlan, 'id' | 'name'>) => spendingPlansApi.remove(plan.id),
    onSuccess: async (_: void, plan) => {
      await refresh();
      toast.success(t('toast.deleted', { name: plan.name }));
    },
    onError: showError,
  });

  /**
   * Tag payments with a plan or clear their tag. `restore` (only the Undo of an assignment)
   * may write a closed plan back. The caller toasts, with its own Undo.
   */
  const setTransactionPlans = useMutation({
    mutationFn: ({
      assignments,
      restore = false,
    }: {
      assignments: SpendingPlanAssignment[];
      restore?: boolean;
    }) => spendingPlansApi.setTransactionPlans(assignments, restore),
    onSuccess: refresh,
    onError: showError,
  });

  return { createPlan, createPlans, updatePlan, setClosed, deletePlan, setTransactionPlans };
}
