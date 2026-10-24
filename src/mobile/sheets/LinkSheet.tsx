import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { PickRow } from '@/mobile/components/PickRow';
import { Sheet } from '@/mobile/components/Sheet';

/** One loan or policy the sheet offers: its name and a second line ("2 400 000 Kč", "Kooperativa · Majetek"). */
export interface LinkCandidate {
  id: string;
  label: string;
  sub?: string;
}

interface LinkSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  kind: 'loan' | 'insurance';
  /** What is not linked to any property yet (`useLinkableLoans`, `useLinkablePolicies`). */
  candidates: readonly LinkCandidate[];
  isLoading: boolean;
  /** A link is being saved: a second tap does nothing. */
  isPending: boolean;
  onLink: (id: string) => void;
}

/**
 * "Připojit úvěr" / "Připojit pojistku" (spec §8.8, the phone counterpart of the desktop's
 * `LinkExistingDialog`): one 44 pt row per loan or policy that no property owns yet — a tap links
 * it. Only unlinked ones are offered: linking one that belongs to another property would silently
 * move it. The caller closes the sheet once the link is saved; with nothing to offer the sheet
 * says where to add one.
 */
export function LinkSheet({
  open,
  onOpenChange,
  kind,
  candidates,
  isLoading,
  isPending,
  onLink,
}: LinkSheetProps) {
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const loan = kind === 'loan';

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t(loan ? 'linkDialog.loanTitle' : 'linkDialog.insuranceTitle')}
      description={t(loan ? 'linkDialog.loanDescription' : 'linkDialog.insuranceDescription')}
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
      {isLoading ? (
        <div aria-hidden className="divide-y divide-line-soft">
          {[0, 1, 2].map((i) => (
            <div key={i} className="flex min-h-11 items-center px-1 py-2.5">
              <div className="skeleton h-3.5 w-2/5" />
            </div>
          ))}
        </div>
      ) : candidates.length === 0 ? (
        <p className="mx-0.5 my-3 text-center text-m-sub text-ink-3">
          {t(loan ? 'linkDialog.noLoans' : 'linkDialog.noInsurances')}
        </p>
      ) : (
        <div className="divide-y divide-line-soft">
          {candidates.map((candidate) => (
            <PickRow
              key={candidate.id}
              label={candidate.label}
              sub={candidate.sub}
              onSelect={() => {
                if (!isPending) onLink(candidate.id);
              }}
            />
          ))}
        </div>
      )}
    </Sheet>
  );
}
