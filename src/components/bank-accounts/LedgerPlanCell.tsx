import { useTranslation } from 'react-i18next';
import { Plus } from 'lucide-react';
import type { SpendingPlan } from '@shared/schema';
import { cn } from '@/lib/utils';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { PlanChip } from '@/components/spending-plans/PlanChip';
import { PlanPicker } from '@/components/spending-plans/PlanPicker';
import { PILL, PILL_INTERACTIVE } from '@/components/common/CategoryPill';

interface LedgerPlanCellProps {
  transactionId: string;
  /** The payment's plan id now. */
  planId: string | null;
  /** That plan from the plan index; undefined while the index loads or when it failed. */
  plan: SpendingPlan | undefined;
  /** The plan index has not answered yet. */
  loading: boolean;
}

/**
 * The "Výdajový plán" cell of the account ledger (spec §9): the plan as a chip (its rounded
 * square icon and name) or, on row hover or focus, a dashed "Přidat do plánu". Both open the
 * plan picker for the row. A closed plan's chip stays, with the tooltip "Uzavřený plán"
 * (spec C5); the picker then offers the active plans and "Odebrat z plánu".
 */
export function LedgerPlanCell({ transactionId, planId, plan, loading }: LedgerPlanCellProps) {
  const { t } = useTranslation('bank_accounts');

  // A failed index falls through to "Přidat do plánu"; its picker still offers "Odebrat z plánu"
  if (planId !== null && !plan && loading) {
    return <span className="skeleton block h-[26px] w-24 rounded-[13px]" aria-hidden />;
  }

  const ids = [transactionId];
  const current = [planId];

  if (!plan) {
    return (
      <PlanPicker
        transactionIds={ids}
        currentPlanIds={current}
        trigger={
          <button
            type="button"
            className={cn(
              PILL,
              PILL_INTERACTIVE,
              'border-dashed border-line-strong text-ink-4 opacity-0 group-hover/row:opacity-100 group-focus-within/row:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100'
            )}
          >
            <Plus className="size-3 shrink-0" aria-hidden />
            <span className="truncate">{t('ledger.addToPlan')}</span>
          </button>
        }
      />
    );
  }

  const closed = plan.closedAt !== null;
  const label = t('ledger.planLabel', { name: plan.name, year: plan.year });
  const chip = (
    <button
      type="button"
      aria-label={closed ? `${label} · ${t('ledger.closedPlan')}` : label}
      title={closed ? undefined : label}
      className={cn(
        PILL,
        PILL_INTERACTIVE,
        'border-line-strong',
        closed ? 'text-ink-3' : 'text-ink-2'
      )}
    >
      <PlanChip plan={plan} size="sm" className="-ml-[5px]" />
      <span className="truncate">{plan.name}</span>
    </button>
  );

  if (!closed) {
    return <PlanPicker transactionIds={ids} currentPlanIds={current} trigger={chip} />;
  }
  return (
    <Tooltip>
      <PlanPicker
        transactionIds={ids}
        currentPlanIds={current}
        trigger={<TooltipTrigger asChild>{chip}</TooltipTrigger>}
      />
      <TooltipContent>{t('ledger.closedPlan')}</TooltipContent>
    </Tooltip>
  );
}
