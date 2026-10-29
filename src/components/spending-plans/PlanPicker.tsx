import { useMemo, useState, type ReactElement } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Plus, Search, X } from 'lucide-react';
import type { SpendingPlan, SpendingPlanSummary } from '@shared/schema';
import { matchesPlanSearch } from '@/utils/spending-plans';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { useSpendingPlanIndex } from '@/hooks/use-spending-plans';
import { useAssignWithUndo } from '@/hooks/use-spending-plan-mutations';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Input, InputWrap } from '@/components/ui/input';
import { PlanChip } from '@/components/spending-plans/PlanChip';
import { PlanFormDialog } from '@/components/spending-plans/PlanFormDialog';

interface PlanPickerProps {
  /** The button the popover opens from. */
  trigger: ReactElement;
  /** The payments to tag. */
  transactionIds: string[];
  /** Each payment's plan now, in the order of `transactionIds`. */
  currentPlanIds: (string | null)[];
  /** A plan not to offer ("Přesunout do…" from that plan's own detail). */
  excludePlanId?: string;
  /** Year of a plan created from here (the page's year); the current year by default. */
  defaultYear?: number;
  /** After the payments were tagged or their tag removed. */
  onAssigned?: () => void;
  align?: 'start' | 'center' | 'end';
}

const ITEM =
  'flex w-full items-start gap-[9px] rounded-r1 px-[9px] py-2 text-left text-table font-550 text-ink-2 outline-none transition-colors duration-fast hover:bg-well hover:text-ink focus-visible:bg-well focus-visible:text-ink disabled:pointer-events-none disabled:opacity-50';

/**
 * The plan picker (spec §9 "Plan picker"): search, the active plans grouped by year (newest
 * first) with what is left of each, a check on the plan all chosen payments share, "Odebrat z
 * plánu" when any of them has a plan and "Nový výdajový plán…", which tags them with the plan
 * once it is created. Every assignment toasts with Undo.
 */
export function PlanPicker({
  trigger,
  transactionIds,
  currentPlanIds,
  excludePlanId,
  defaultYear,
  onAssigned,
  align = 'start',
}: PlanPickerProps) {
  const { t } = useTranslation('spending_plans');
  const fmt = useFormat();
  const index = useSpendingPlanIndex();
  const { assign, isPending } = useAssignWithUndo();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [formOpen, setFormOpen] = useState(false);

  const rows = transactionIds.map((id, i) => ({ id, spendingPlanId: currentPlanIds[i] ?? null }));
  const shared = new Set(currentPlanIds);
  const sharedPlanId = shared.size === 1 ? [...shared][0] : null;
  const anyInPlan = currentPlanIds.some((id) => id !== null);

  const groups = useMemo(() => {
    const active = (index.data ?? []).filter(
      (s) =>
        s.plan.closedAt === null &&
        s.plan.id !== excludePlanId &&
        matchesPlanSearch(s.plan.name, query)
    );
    const years = [...new Set(active.map((s) => s.plan.year))].sort((a, b) => b - a);
    return years.map((year) => ({
      year,
      plans: active
        .filter((s) => s.plan.year === year)
        .sort((a, b) => a.plan.name.localeCompare(b.plan.name, fmt.locale)),
    }));
  }, [index.data, excludePlanId, query, fmt.locale]);

  const changeOpen = (next: boolean) => {
    setOpen(next);
    if (!next) setQuery('');
  };

  const pick = async (plan: Pick<SpendingPlan, 'id' | 'name'> | null) => {
    changeOpen(false);
    // Picking the plan every payment already has changes nothing
    if (plan && plan.id === sharedPlanId) return;
    if (await assign(rows, plan)) onAssigned?.();
  };

  const remaining = (summary: SpendingPlanSummary) => {
    const planned = parseFloat(summary.plan.amount) || 0;
    const money = (value: number) => fmt.money(value, summary.plan.currency);
    if (summary.spent === null) return t('picker.planned', { planned: money(planned) });
    const left = planned - parseFloat(summary.spent);
    return left < 0
      ? t('picker.over', { amount: money(-left) })
      : t('picker.left', { left: money(left), planned: money(planned) });
  };

  return (
    <>
      <Popover open={open} onOpenChange={changeOpen}>
        <PopoverTrigger asChild>{trigger}</PopoverTrigger>
        <PopoverContent
          align={align}
          collisionPadding={8}
          aria-label={t('picker.label')}
          className="flex max-h-[var(--radix-popover-content-available-height)] w-[310px] flex-col p-1.5"
        >
          <InputWrap icon={<Search />} className="flex-none">
            <Input
              autoFocus
              type="search"
              className="h-8 text-table"
              placeholder={t('picker.search')}
              aria-label={t('picker.search')}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </InputWrap>
          {/* The wheel must reach the list when the picker sits over a dialog (Tauri webview) */}
          <div className="mt-1 min-h-0 flex-1 overflow-y-auto" onWheel={(e) => e.stopPropagation()}>
            {groups.length === 0 ? (
              <p className="m-0 px-[9px] py-3.5 text-table text-ink-4">
                {query.trim()
                  ? t('picker.noMatch', { query: query.trim() })
                  : index.isLoading
                    ? '…'
                    : t('picker.none')}
              </p>
            ) : (
              groups.map((group) => (
                <div key={group.year} role="group" aria-label={String(group.year)}>
                  <div className="px-[9px] pb-1 pt-[9px] text-micro font-700 uppercase tracking-[0.06em] text-ink-4">
                    {group.year}
                  </div>
                  {group.plans.map((summary) => {
                    const current = summary.plan.id === sharedPlanId;
                    return (
                      <button
                        key={summary.plan.id}
                        type="button"
                        className={ITEM}
                        disabled={isPending}
                        aria-current={current || undefined}
                        onClick={() => void pick(summary.plan)}
                      >
                        <PlanChip plan={summary.plan} size="sm" className="mt-px" />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate">{summary.plan.name}</span>
                          <small className="mt-0.5 block text-micro font-500 text-ink-4 num">
                            {remaining(summary)}
                          </small>
                        </span>
                        {current && (
                          <Check
                            className="ml-auto mt-0.5 size-3.5 shrink-0 text-ink-2"
                            aria-hidden
                          />
                        )}
                      </button>
                    );
                  })}
                </div>
              ))
            )}
          </div>
          <p className="m-0 flex-none px-[9px] pb-1 pt-1.5 text-micro text-ink-4">
            {t('picker.closedNote')}
          </p>
          <div className="mx-1 my-[5px] h-px flex-none bg-line-soft" />
          {anyInPlan && (
            <button
              type="button"
              className={cn(ITEM, 'flex-none items-center')}
              disabled={isPending}
              onClick={() => void pick(null)}
            >
              <X className="size-3.5 text-ink-3" aria-hidden />
              {t('actions.removeFromPlan')}
            </button>
          )}
          <button
            type="button"
            className={cn(ITEM, 'flex-none items-center')}
            onClick={() => {
              changeOpen(false);
              setFormOpen(true);
            }}
          >
            <Plus className="size-3.5 text-ink-3" aria-hidden />
            {t('actions.newPlanEllipsis')}
          </button>
        </PopoverContent>
      </Popover>
      <PlanFormDialog
        open={formOpen}
        onOpenChange={setFormOpen}
        defaultYear={defaultYear ?? new Date().getFullYear()}
        onCreated={(plan) => void pick(plan)}
      />
    </>
  );
}
