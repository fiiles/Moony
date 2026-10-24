import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Ellipsis, Pencil, Repeat, Trash2 } from 'lucide-react';
import type { InsertLoan, Loan, LoanEventKind } from '@shared/schema';
import { useLoanMutations } from '@/hooks/use-loan-mutations';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { LoanFormDialog } from '@/components/loans/LoanFormDialog';
import { leaveAfterSheet } from '@/mobile/hooks/use-leave-after-sheet';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { NavIconButton } from '@/mobile/shell/NavBar';

/** The classes of a menu item on the phone: 44 pt tall, body type, 18 pt icons. */
const MENU_ITEM = 'min-h-11 text-m-body [&_svg]:size-[18px]';

interface LoanMenuProps {
  loan: Loan;
  /** Zadat zůstatek k datu and Zapsat změnu sazby: the screen's event dialog (it is also the extra payment's). */
  onOpenEvent: (kind: Extract<LoanEventKind, 'balance_check' | 'rate_change'>) => void;
  /** The loan is deleted and its confirm sheet has closed: leave the screen. */
  onDeleted: () => void;
}

/**
 * The `···` of a loan (spec §8.8): Upravit opens the desktop's loan form (saved through the loan
 * mutations), Zadat zůstatek k datu and Zapsat změnu sazby ask the screen to open the event dialog,
 * Smazat úvěr asks in a confirm sheet and, once the loan is gone and the sheet's history entry with
 * it, tells the page to leave (`onDeleted`).
 */
export function LoanMenu({ loan, onOpenEvent, onDeleted }: LoanMenuProps) {
  const { t: tl } = useTranslation('loans');
  const { t: tc } = useTranslation('common');
  const { updateMutation, deleteMutation } = useLoanMutations();
  const [editOpen, setEditOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);

  const save = (data: InsertLoan | (Partial<Loan> & { id: string })) => {
    if (!('id' in data)) return;
    updateMutation.mutate(data as { id: string } & Partial<InsertLoan>, {
      onSuccess: () => setEditOpen(false),
    });
  };

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <NavIconButton aria-label={tc('labels.moreActions')}>
            <Ellipsis strokeWidth={1.75} aria-hidden />
          </NavIconButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-[240px]">
          <DropdownMenuItem className={MENU_ITEM} onSelect={() => setEditOpen(true)}>
            <Pencil strokeWidth={1.75} aria-hidden />
            {tl('actions.edit')}
          </DropdownMenuItem>
          <DropdownMenuItem className={MENU_ITEM} onSelect={() => onOpenEvent('balance_check')}>
            <Check strokeWidth={1.75} aria-hidden />
            {tl('actions.balanceCheck')}
          </DropdownMenuItem>
          <DropdownMenuItem className={MENU_ITEM} onSelect={() => onOpenEvent('rate_change')}>
            <Repeat strokeWidth={1.75} aria-hidden />
            {tl('actions.rateChange')}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            variant="danger"
            className={MENU_ITEM}
            onSelect={() => setDeleteOpen(true)}
          >
            <Trash2 strokeWidth={1.75} aria-hidden />
            {tl('actions.delete')}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <LoanFormDialog
        open={editOpen}
        onOpenChange={setEditOpen}
        onSubmit={save}
        loan={loan}
        isLoading={updateMutation.isPending}
      />
      <ConfirmSheet
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={tl('confirmDelete.title')}
        description={tl('confirmDelete.description', { name: loan.name })}
        confirmLabel={tl('actions.delete')}
        isPending={deleteMutation.isPending}
        onConfirm={() =>
          deleteMutation.mutate(loan.id, {
            // The sheet takes its own history entry off first, then the page goes back
            onSuccess: () => leaveAfterSheet(() => setDeleteOpen(false), onDeleted),
          })
        }
      />
    </>
  );
}
