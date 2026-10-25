import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Pencil, Receipt, Repeat, Trash2 } from 'lucide-react';
import type { RealEstate, RealEstateOneTimeCost } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import type { YearlyBalance } from '@/utils/real-estate';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { RowMenu, RowMenuItem } from '@/mobile/components/cashflow/RowMenu';
import { SectionHead } from '@/mobile/components/SectionHead';
import { GroupHead } from '@/mobile/components/property/GroupHead';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { usePropertyCostMutations } from '@/mobile/hooks/use-property-mutations';
import { costRows, type OneTimeCostRow, type RecurringCostRow } from '@/mobile/lib/property-detail';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { CostSheet } from '@/mobile/sheets/CostSheet';

/** Whole amounts: the head's note shows no decimals. */
const WHOLE = { decimals: 0 } as const;

/** The cost a confirm sheet is asking about: a recurring one by its place, a one-time one by itself. */
type PendingDelete =
  { kind: 'recurring'; row: RecurringCostRow } | { kind: 'oneTime'; row: OneTimeCostRow };

interface PropertyCostsProps {
  property: RealEstate;
  /** The one-time costs (`['real-estate-costs', id]`). */
  costs: readonly RealEstateOneTimeCost[];
  balance: YearlyBalance;
  /** The main currency. */
  currency: string;
}

/**
 * "Náklady" of a property (spec §8.8 "Nemovitost"; the desktop's Náklady tab and its one-time
 * costs): the head carries what the recurring costs come to in a year in the main currency (masked,
 * "—" while a currency has no rate, left out without recurring costs); then two groups, the
 * recurring costs (the name, how often, one payment and the payments of a year) and the history
 * of one-time costs (the name, the day with its note, the amount), newest first, each with the
 * desktop's line when empty. Every cost is in its own currency, never converted. Přidat náklad
 * opens the cost sheet; a tap on a one-time cost edits it, a long press opens the row menu —
 * Upravit and Smazat for a one-time cost, Smazat for a recurring one (a tap opens its menu) — and
 * Smazat asks in a confirm sheet first. Every amount goes through the eye; days and counts stay.
 */
export function PropertyCosts({ property, costs, balance, currency }: PropertyCostsProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { removeOneTime, removeRecurring } = usePropertyCostMutations(property.id);

  const rows = costRows(property, costs, { money: moneyText, day: fmt.day });

  const [menuFor, setMenuFor] = useState<string | null>(null);
  // The sheets keep their cost while they slide away: which one and whether it is open are apart
  const [editing, setEditing] = useState<RealEstateOneTimeCost | null>(null);
  const [sheetOpen, setSheetOpen] = useState(false);
  const [pending, setPending] = useState<PendingDelete | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);

  const add = () => {
    setEditing(null);
    setSheetOpen(true);
  };
  const edit = (cost: RealEstateOneTimeCost) => {
    setEditing(cost);
    setSheetOpen(true);
  };
  const ask = (next: PendingDelete) => {
    setPending(next);
    setConfirmOpen(true);
  };
  const frequency = (value: string) => tr(`detail.frequency.${value}`, { defaultValue: value });

  const confirmed = () => {
    if (!pending) return;
    const done = { onSuccess: () => setConfirmOpen(false) };
    if (pending.kind === 'recurring') {
      removeRecurring.mutate({ property, index: pending.row.index }, done);
    } else {
      removeOneTime.mutate(pending.row.id, done);
    }
  };

  const note = balance.hasRecurring
    ? tr('detail.recurringTable.total', {
        amount: balance.recurring !== null ? moneyText(balance.recurring, currency, WHOLE) : '—',
      })
    : undefined;

  return (
    <>
      <SectionHead title={tr('detail.tabs.costs')} note={note} />

      <GroupHead
        title={tr('detail.recurringCosts')}
        action={{ label: tr('modal.cost.addCost'), onClick: add }}
      />
      {rows.recurring.length === 0 ? (
        <p className="mx-0.5 mb-3.5 text-m-sub text-ink-3">{tr('detail.noRecurringCosts')}</p>
      ) : (
        <EntityRows>
          {rows.recurring.map((row) => {
            const key = `recurring-${row.index}`;
            return (
              <EntityRow
                key={key}
                chip={
                  <RowChip>
                    <Repeat strokeWidth={1.75} />
                  </RowChip>
                }
                title={row.name}
                sub={frequency(row.frequency)}
                value={row.amount}
                // A year's payments, when a payment is not a year's already
                valueSub={
                  row.perYear > 1 ? t('property.costYear', { amount: row.yearly }) : undefined
                }
                onClick={() => setMenuFor(key)}
                onLongPress={() => setMenuFor(key)}
                accessory={
                  <RowMenu
                    open={menuFor === key}
                    onOpenChange={(next) => setMenuFor(next ? key : null)}
                    label={t('property.rowMenu', { name: row.name })}
                  >
                    <RowMenuItem variant="danger" onSelect={() => ask({ kind: 'recurring', row })}>
                      <Trash2 strokeWidth={1.75} aria-hidden />
                      {tc('buttons.delete')}
                    </RowMenuItem>
                  </RowMenu>
                }
              />
            );
          })}
        </EntityRows>
      )}

      <GroupHead title={tr('detail.oneTimeCosts')} />
      {rows.oneTime.length === 0 ? (
        <p className="mx-0.5 mb-3.5 text-m-sub text-ink-3">{tr('detail.noOneTimeCosts')}</p>
      ) : (
        <EntityRows>
          {rows.oneTime.map((row) => {
            const key = `one-time-${row.id}`;
            return (
              <EntityRow
                key={key}
                chip={
                  <RowChip>
                    <Receipt strokeWidth={1.75} />
                  </RowChip>
                }
                title={row.name}
                sub={[row.day, row.description].filter(Boolean).join(' · ')}
                value={row.amount}
                onClick={() => edit(row.cost)}
                onLongPress={() => setMenuFor(key)}
                accessory={
                  <RowMenu
                    open={menuFor === key}
                    onOpenChange={(next) => setMenuFor(next ? key : null)}
                    label={t('property.rowMenu', { name: row.name })}
                  >
                    <RowMenuItem onSelect={() => edit(row.cost)}>
                      <Pencil strokeWidth={1.75} aria-hidden />
                      {tc('buttons.edit')}
                    </RowMenuItem>
                    <RowMenuItem variant="danger" onSelect={() => ask({ kind: 'oneTime', row })}>
                      <Trash2 strokeWidth={1.75} aria-hidden />
                      {tc('buttons.delete')}
                    </RowMenuItem>
                  </RowMenu>
                }
              />
            );
          })}
        </EntityRows>
      )}

      <CostSheet open={sheetOpen} onOpenChange={setSheetOpen} property={property} cost={editing} />
      <ConfirmSheet
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={tr('confirmDeleteCost.title')}
        description={
          pending?.kind === 'recurring'
            ? t('property.confirmDeleteRecurring', {
                name: pending.row.name,
                amount: pending.row.amount,
                frequency: frequency(pending.row.frequency),
              })
            : pending
              ? tr('confirmDeleteCost.description', {
                  name: pending.row.name,
                  date: pending.row.day,
                  amount: pending.row.amount,
                })
              : ''
        }
        confirmLabel={tc('buttons.delete')}
        isPending={removeOneTime.isPending || removeRecurring.isPending}
        onConfirm={confirmed}
      />
    </>
  );
}
