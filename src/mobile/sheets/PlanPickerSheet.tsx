import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, X } from 'lucide-react';
import type { BankTransaction, SpendingPlan } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useSpendingPlanIndex } from '@/hooks/use-spending-plans';
import { useAssignWithUndo } from '@/hooks/use-spending-plan-mutations';
import { useFormat } from '@/lib/use-format';
import { NeutralChip, PickGroup, PickRow } from '@/mobile/components/PickRow';
import { PlanRowChip } from '@/mobile/components/plans/PlanChip';
import { SearchField } from '@/mobile/components/SearchField';
import { Sheet } from '@/mobile/components/Sheet';
import { useRetained } from '@/mobile/hooks/use-retained';
import { SEARCH_FROM, bookingYear, pickerGroups } from '@/mobile/lib/plan-rows';
import { PlanFormSheet } from '@/mobile/sheets/PlanFormSheet';

type PickerTx = Pick<BankTransaction, 'id' | 'spendingPlanId' | 'bookingDate'>;

interface PlanPickerSheetProps {
  /** The payment to tag; `null` while the sheet is closed (it keeps the last one while it slides away). */
  tx: PickerTx | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The payment now has this plan (null: none), so the caller can patch its copy of the row. */
  onAssigned?: (planId: string | null) => void;
}

/**
 * The plan picker (spec M12): the open plans of the payment's booking year first, then those of
 * other years under their year, each with what is left in the plan's own currency, a check on the
 * current plan, "Bez plánu" first when the payment has one and "Nový výdajový plán…" last. With
 * `SEARCH_FROM` plans or more on offer it has a search field. A choice is saved at once through
 * `useAssignWithUndo` (toast with Vrátit zpět) and closes the sheet.
 */
export function PlanPickerSheet({ tx, open, onOpenChange, onAssigned }: PlanPickerSheetProps) {
  const { t: tc } = useTranslation('common');
  const { t } = useTranslation('mobile');
  const shown = useRetained(tx);

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('transaction.plan')}
      full
      footer={
        <Button
          variant="ghost"
          className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
          onClick={() => onOpenChange(false)}
        >
          {tc('buttons.cancel')}
        </Button>
      }
    >
      {/* Its own component: the search starts over every time the sheet opens */}
      {shown && (
        <PickerBody tx={shown} onClose={() => onOpenChange(false)} onAssigned={onAssigned} />
      )}
    </Sheet>
  );
}

function PickerBody({
  tx,
  onClose,
  onAssigned,
}: {
  tx: PickerTx;
  onClose: () => void;
  onAssigned?: (planId: string | null) => void;
}) {
  const { t } = useTranslation('mobile');
  const { t: tp } = useTranslation('spending_plans');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const index = useSpendingPlanIndex();
  const { assign, isPending } = useAssignWithUndo();
  const [query, setQuery] = useState('');
  const [formOpen, setFormOpen] = useState(false);

  const year = bookingYear(tx.bookingDate);
  const offered = pickerGroups(index.data ?? [], year, '', fmt.locale);
  const offeredCount = offered.reduce((sum, group) => sum + group.rows.length, 0);
  const groups = pickerGroups(index.data ?? [], year, query, fmt.locale);
  const searching = query.trim() !== '';

  const choose = async (plan: Pick<SpendingPlan, 'id' | 'name'> | null) => {
    if (isPending) return;
    // Picking the plan the payment already has changes nothing
    if (plan && plan.id === tx.spendingPlanId) {
      onClose();
      return;
    }
    if (await assign([tx], plan)) {
      onAssigned?.(plan?.id ?? null);
      onClose();
    }
  };

  return (
    <>
      {offeredCount >= SEARCH_FROM && (
        <SearchField
          value={query}
          onChange={setQuery}
          placeholder={tp('picker.search')}
          clearLabel={tc('a11y.clearSearch')}
        />
      )}
      {tx.spendingPlanId !== null && !searching && (
        <PickRow
          icon={<NeutralChip icon={<X />} />}
          label={t('transaction.noPlan')}
          onSelect={() => void choose(null)}
        />
      )}
      {groups.map((group) => (
        <PickGroup key={group.year} title={String(group.year)}>
          {group.rows.map((row) => {
            const { plan } = row.summary;
            const money = (value: number) => fmt.money(value, plan.currency, { decimals: 0 });
            const sub =
              row.left === null
                ? '—'
                : row.left < 0
                  ? tp('picker.over', { amount: money(-row.left) })
                  : tp('picker.left', { left: money(row.left), planned: money(row.planned) });
            return (
              <PickRow
                key={plan.id}
                icon={<PlanRowChip plan={plan} />}
                label={plan.name}
                sub={sub}
                checked={plan.id === tx.spendingPlanId}
                onSelect={() => void choose(plan)}
              />
            );
          })}
        </PickGroup>
      ))}
      {groups.length === 0 && (
        <p className="mx-1 my-3 text-m-body text-ink-3">
          {searching
            ? tp('picker.noMatch', { query: query.trim() })
            : index.isLoading
              ? '…'
              : tp('picker.none')}
        </p>
      )}
      <PickRow
        icon={<NeutralChip icon={<Plus />} />}
        label={tp('actions.newPlanEllipsis')}
        onSelect={() => setFormOpen(true)}
      />
      <PlanFormSheet
        open={formOpen}
        onOpenChange={setFormOpen}
        defaultYear={year}
        onSaved={(plan) => void choose(plan)}
      />
    </>
  );
}
