import { useTranslation } from 'react-i18next';
import { FolderInput, X } from 'lucide-react';
import type { BankTransaction } from '@shared/schema';
import { signedTotals } from '@/utils/spending-plans';
import { useAssignWithUndo } from '@/hooks/use-spending-plan-mutations';
import { useFormat } from '@/lib/use-format';
import { Button } from '@/components/ui/button';
import { PlanPicker } from '@/components/spending-plans/PlanPicker';

interface LedgerBulkBarProps {
  /** The selected payments, in the ledger's order (never empty while the bar shows). */
  rows: BankTransaction[];
  /** After the payments were tagged or untagged, and on "Zrušit výběr". */
  onClear: () => void;
}

/**
 * The account ledger's bulk bar (spec §9): "Vybrány n platby · ± sum" (a sum per currency when
 * the payments mix currencies, never converted), primary "Přidat do plánu" (the plan picker),
 * "Odebrat z plánu" (only payments that have a plan; disabled when none has) and "Zrušit výběr".
 */
export function LedgerBulkBar({ rows, onClear }: LedgerBulkBarProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tp } = useTranslation('spending_plans');
  const fmt = useFormat();
  const { assign, isPending } = useAssignWithUndo();

  const inPlan = rows.filter((tx) => tx.spendingPlanId !== null);
  const totals = signedTotals(rows)
    .map((s) => fmt.money(s.amount, s.currency, { signed: true }))
    .join(' · ');

  return (
    <div className="flex min-h-[52px] items-center gap-2 border-t border-line bg-well px-[14px] py-2 text-table">
      <b className="shrink-0 text-ink">{tp('selection.count', { count: rows.length })}</b>
      <span className="mr-auto min-w-0 truncate text-ink-3 num" title={totals}>
        · {totals}
      </span>
      <PlanPicker
        trigger={
          <Button size="sm" disabled={isPending}>
            <FolderInput />
            {t('ledger.addToPlan')}
          </Button>
        }
        transactionIds={rows.map((tx) => tx.id)}
        currentPlanIds={rows.map((tx) => tx.spendingPlanId)}
        align="end"
        onAssigned={onClear}
      />
      <Button
        variant="outline"
        size="sm"
        disabled={isPending || inPlan.length === 0}
        onClick={() =>
          void assign(inPlan, null).then((ok) => {
            if (ok) onClear();
          })
        }
      >
        <X />
        {tp('actions.removeFromPlan')}
      </Button>
      <Button variant="ghost" size="sm" onClick={onClear}>
        {tp('actions.clearSelection')}
      </Button>
    </div>
  );
}
