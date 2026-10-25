import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Ellipsis, Pencil, Repeat, Scale, Trash2 } from 'lucide-react';
import type { InsertLoan, Loan, LoanEventKind } from '@shared/schema';
import { useLoanMutations } from '@/hooks/use-loan-mutations';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { LoanFormDialog } from '@/components/loans/LoanFormDialog';
import { NavIconButton } from '@/mobile/shell/NavBar';

interface LoanMenuProps {
  loan: Loan;
  /** Zadat zůstatek k datu and Zapsat změnu sazby: the screen's event dialog (it is also the extra payment's). */
  onOpenEvent: (kind: Extract<LoanEventKind, 'balance_check' | 'rate_change'>) => void;
  /** Smazat úvěr: the screen asks in its confirm sheet and deletes (`useDeleteAndLeave`). */
  onDelete: () => void;
}

/**
 * The `···` of a loan (spec §8.8): Upravit opens the desktop's loan form (saved through the loan
 * mutations), Zadat zůstatek k datu and Zapsat změnu sazby ask the screen to open the event dialog,
 * Smazat úvěr asks the screen to confirm and delete. The delete is the screen's, not this menu's:
 * the menu goes away as soon as the loan drops out of the list, taking its callbacks with it.
 */
export function LoanMenu({ loan, onOpenEvent, onDelete }: LoanMenuProps) {
  const { t: tl } = useTranslation('loans');
  const { t: tc } = useTranslation('common');
  const { updateMutation } = useLoanMutations();
  const [editOpen, setEditOpen] = useState(false);

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
          <RowMenuItem onSelect={() => setEditOpen(true)}>
            <Pencil strokeWidth={1.75} aria-hidden />
            {tl('actions.edit')}
          </RowMenuItem>
          <RowMenuItem onSelect={() => onOpenEvent('balance_check')}>
            <Scale strokeWidth={1.75} aria-hidden />
            {tl('actions.balanceCheck')}
          </RowMenuItem>
          <RowMenuItem onSelect={() => onOpenEvent('rate_change')}>
            <Repeat strokeWidth={1.75} aria-hidden />
            {tl('actions.rateChange')}
          </RowMenuItem>
          <DropdownMenuSeparator />
          <RowMenuItem variant="danger" onSelect={onDelete}>
            <Trash2 strokeWidth={1.75} aria-hidden />
            {tl('actions.delete')}
          </RowMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <LoanFormDialog
        open={editOpen}
        onOpenChange={setEditOpen}
        onSubmit={save}
        loan={loan}
        isLoading={updateMutation.isPending}
      />
    </>
  );
}
