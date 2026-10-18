import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { BudgetGoalInterval } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useDeleteBudget, useDeleteBudgetIntervals } from '@/hooks/use-budget-goal-mutations';
import { Sheet } from '@/mobile/components/Sheet';

/** What is waiting for a confirmation: a whole budget, or one interval of it. */
export type PendingDelete =
  | { kind: 'budget'; categoryId: string; timeframe: string; name: string }
  | {
      kind: 'interval';
      interval: BudgetGoalInterval;
      name: string;
      /** The interval's limit and validity, worded for the sentence ("3 000 Kč", "leden 2026 – červen 2026"). */
      amount: string;
      period: string;
    };

interface BudgetDeleteSheetProps {
  /** What to delete; `null` closes the sheet (it keeps the last one while it slides away). */
  pending: PendingDelete | null;
  onClose: () => void;
}

/**
 * The confirmation before a budget or one of its intervals goes (the phone counterpart of the
 * desktop's two `ConfirmDeleteDialog`s, README §13 Confirm sheet): the question as the title, the
 * sentence that says what else goes (the past months of the spending reports) under it, a ghost
 * "Zrušit" and the solid red button — the one place a destructive button belongs. The button
 * names the deed, as the desktop's does ("Smazat celý rozpočet", "Smazat").
 */
export function BudgetDeleteSheet({ pending, onClose }: BudgetDeleteSheetProps) {
  const [shown, setShown] = useState(pending);
  if (pending !== null && pending !== shown) setShown(pending);
  const budget = useDeleteBudget(onClose);
  const intervals = useDeleteBudgetIntervals(onClose);
  const { t } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');

  const wholeBudget = shown?.kind !== 'interval';
  const isPending = wholeBudget ? budget.isPending : intervals.isPending;
  const confirm = () => {
    if (!shown || isPending) return;
    if (shown.kind === 'budget') {
      budget.mutate({ categoryId: shown.categoryId, timeframe: shown.timeframe });
    } else {
      intervals.mutate([shown.interval]);
    }
  };

  return (
    <Sheet
      open={pending !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={wholeBudget ? t('manage.deleteTitle') : t('manage.deleteIntervalTitle')}
      description={
        shown === null
          ? undefined
          : shown.kind === 'budget'
            ? t('manage.deleteDescription', { category: shown.name })
            : t('manage.deleteIntervalDescription', {
                category: shown.name,
                amount: shown.amount,
                period: shown.period,
              })
      }
      footer={
        <>
          <Button
            variant="ghost"
            className="h-12 rounded-[10px] px-[18px] text-m-row"
            onClick={onClose}
          >
            {tc('buttons.cancel')}
          </Button>
          <Button
            variant="destructive"
            className="h-12 flex-1 rounded-[10px] text-m-row"
            loading={isPending}
            onClick={confirm}
          >
            {wholeBudget ? t('manage.delete') : tc('buttons.delete')}
          </Button>
        </>
      }
    >
      {null}
    </Sheet>
  );
}
