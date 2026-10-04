import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { cn } from '@/lib/utils';

export interface LinkCandidate {
  id: string;
  label: string;
  meta?: string;
}

interface LinkExistingDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: string;
  candidates: LinkCandidate[];
  emptyText: string;
  linkLabel: string;
  isLoading?: boolean;
  isPending?: boolean;
  onLink: (id: string) => void;
}

/**
 * Pick one existing record (loan, insurance policy) to link to a property
 * (RED-02). The page owns the candidate list and the mutation.
 */
export function LinkExistingDialog({
  open,
  onOpenChange,
  title,
  description,
  candidates,
  emptyText,
  linkLabel,
  isLoading = false,
  isPending = false,
  onLink,
}: LinkExistingDialogProps) {
  const { t: tc } = useTranslation('common');
  const [selected, setSelected] = useState<string | null>(null);

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) setSelected(null);
        onOpenChange(next);
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>

        {isLoading ? (
          <div className="flex items-center gap-2 text-sm text-ink-3 py-6">
            <Loader2 className="h-4 w-4 animate-spin" />
            {tc('status.loading')}
          </div>
        ) : candidates.length === 0 ? (
          <p className="text-sm text-ink-3 py-6 text-center">{emptyText}</p>
        ) : (
          <ul
            className="max-h-72 divide-y divide-line overflow-y-auto rounded-r2 border border-line"
            role="listbox"
          >
            {candidates.map((c) => (
              <li key={c.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={selected === c.id}
                  onClick={() => setSelected(c.id)}
                  className={cn(
                    'w-full px-3 py-2 text-left text-sm transition-colors hover:bg-well',
                    selected === c.id && 'bg-well-2'
                  )}
                >
                  <div className="font-medium">{c.label}</div>
                  {c.meta && <div className="text-xs text-ink-3">{c.meta}</div>}
                </button>
              </li>
            ))}
          </ul>
        )}

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={isPending}>
            {tc('buttons.cancel')}
          </Button>
          <Button
            onClick={() => selected && onLink(selected)}
            disabled={!selected || isPending || candidates.length === 0}
            loading={isPending}
          >
            {isPending ? tc('status.saving') : linkLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
