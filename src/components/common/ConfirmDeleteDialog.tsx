import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';

interface ConfirmDeleteDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: string;
  onConfirm: () => void;
  /** Disables both buttons and shows the "Deleting..." label while the mutation runs. */
  isPending?: boolean;
  /** The confirming verb ("Smazat pozici"); defaults to the generic "Smazat". */
  confirmLabel?: string;
}

/**
 * Generic "are you sure?" dialog for deleting a single row (transaction, cost, ...).
 *
 * The page owns the state: it keeps the row it is about to delete (`pendingDelete`),
 * derives `open` from it, and runs the mutation in `onConfirm`. The dialog does NOT
 * close itself on confirm — the page clears `pendingDelete` once the mutation
 * succeeds, so a failed delete leaves the dialog (and the error toast) visible.
 */
export function ConfirmDeleteDialog({
  open,
  onOpenChange,
  title,
  description,
  onConfirm,
  isPending = false,
  confirmLabel,
}: ConfirmDeleteDialogProps) {
  const { t: tc } = useTranslation('common');

  // The page clears `pendingDelete` when the dialog closes, which would blank the
  // text while the exit animation is still playing. Keep the last open text around.
  const [lastShown, setLastShown] = useState({ title, description });
  if (open && (lastShown.title !== title || lastShown.description !== description)) {
    setLastShown({ title, description });
  }
  const shown = open ? { title, description } : lastShown;

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{shown.title}</AlertDialogTitle>
          <AlertDialogDescription>{shown.description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={isPending}>{tc('buttons.cancel')}</AlertDialogCancel>
          <AlertDialogAction
            onClick={(e) => {
              // Keep the dialog open until the page confirms the delete succeeded.
              e.preventDefault();
              onConfirm();
            }}
            disabled={isPending}
          >
            {isPending ? tc('status.deleting') : (confirmLabel ?? tc('buttons.delete'))}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
