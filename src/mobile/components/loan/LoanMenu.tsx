import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Ellipsis, Pencil, Repeat, Scale, Trash2 } from 'lucide-react';
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
import { NavIconButton } from '@/mobile/shell/NavBar';

/** The classes of a menu item on the phone: 44 pt tall, body type, 18 pt icons. */
const MENU_ITEM = 'min-h-11 text-m-body [&_svg]:size-[18px]';

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
          <DropdownMenuItem className={MENU_ITEM} onSelect={() => setEditOpen(true)}>
            <Pencil strokeWidth={1.75} aria-hidden />
            {tl('actions.edit')}
          </DropdownMenuItem>
          <DropdownMenuItem className={MENU_ITEM} onSelect={() => onOpenEvent('balance_check')}>
            <Scale strokeWidth={1.75} aria-hidden />
            {tl('actions.balanceCheck')}
          </DropdownMenuItem>
          <DropdownMenuItem className={MENU_ITEM} onSelect={() => onOpenEvent('rate_change')}>
            <Repeat strokeWidth={1.75} aria-hidden />
            {tl('actions.rateChange')}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="danger" className={MENU_ITEM} onSelect={onDelete}>
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
    </>
  );
}
