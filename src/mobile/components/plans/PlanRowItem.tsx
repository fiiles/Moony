import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ArchiveRestore, Lock, Pencil, Trash2 } from 'lucide-react';
import type { SpendingPlan } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { ProgressBar } from '@/mobile/components/cashflow/ProgressBar';
import { PlanRowChip } from '@/mobile/components/plans/PlanChip';
import { EntityRow } from '@/mobile/components/EntityRow';
import { RowMenu, RowMenuItem } from '@/mobile/components/RowMenu';
import { planPath } from '@/mobile/shell/routes';
import type { PlanRow } from '@/utils/spending-plans';

interface PlanRowItemProps {
  row: PlanRow;
  onEdit: (plan: SpendingPlan) => void;
  onToggleClosed: (plan: SpendingPlan) => void;
  onDelete: (row: PlanRow) => void;
}

/**
 * One plan of the list (spec M5): the square plan chip, the name, "Utraceno X z Y" in the plan
 * currency and the usage bar; the value is "zbývá X", or "přes o X" in `loss`, a status word under
 * it ("Nad plánem", "Blízko plánu", "Uzavřený"). A spend that could not be converted shows "—",
 * never 0. A tap opens the plan; there are no swipe actions, so a plan is never deleted by a
 * stray swipe. The long press's menu: Upravit, Uzavřít / Obnovit plán, Smazat plán.
 */
export function PlanRowItem({ row, onEdit, onToggleClosed, onDelete }: PlanRowItemProps) {
  const { t } = useTranslation('mobile');
  const { t: tp } = useTranslation('spending_plans');
  const fmt = useFormat();
  const [menuOpen, setMenuOpen] = useState(false);
  const { plan } = row.summary;
  const money = (value: number) => fmt.money(value, plan.currency, { decimals: 0 });

  const over = row.left !== null && row.left < 0;
  const value =
    row.left === null
      ? '—'
      : over
        ? t('plans.row.over', { amount: money(-row.left) })
        : t('plans.row.left', { amount: money(row.left) });
  const valueSub = row.closed
    ? tp('status.closed')
    : row.status === 'over' || row.status === 'near' || row.status === 'unknown'
      ? tp(`status.${row.status}`)
      : undefined;
  const bar =
    row.ratio === null ? undefined : (
      <ProgressBar value={row.ratio} tone={row.ratio > 1 ? 'over' : 'limit'} />
    );

  return (
    <EntityRow
      chip={<PlanRowChip plan={plan} />}
      title={plan.name}
      sub={t('plans.row.spent', {
        spent: row.spent === null ? '—' : money(row.spent),
        planned: money(row.planned),
      })}
      value={value}
      valueTone={over ? 'loss' : undefined}
      valueSub={valueSub}
      bar={bar}
      muted={row.closed}
      href={planPath(plan.id)}
      onLongPress={() => setMenuOpen(true)}
      accessory={
        <RowMenu
          open={menuOpen}
          onOpenChange={setMenuOpen}
          label={t('cashflow.rowMenu', { title: plan.name })}
        >
          <RowMenuItem onSelect={() => onEdit(plan)}>
            <Pencil aria-hidden strokeWidth={1.75} />
            {tp('actions.edit')}
          </RowMenuItem>
          <RowMenuItem onSelect={() => onToggleClosed(plan)}>
            {row.closed ? (
              <ArchiveRestore aria-hidden strokeWidth={1.75} />
            ) : (
              <Lock aria-hidden strokeWidth={1.75} />
            )}
            {row.closed ? tp('actions.reopen') : tp('actions.close')}
          </RowMenuItem>
          <RowMenuItem variant="danger" onSelect={() => onDelete(row)}>
            <Trash2 aria-hidden strokeWidth={1.75} />
            {tp('actions.delete')}
          </RowMenuItem>
        </RowMenu>
      }
    />
  );
}
