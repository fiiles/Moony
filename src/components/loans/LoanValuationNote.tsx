import { Info, TriangleAlert } from 'lucide-react';
import type { Loan } from '@shared/schema';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useLoanValuationNote } from '@/hooks/use-loan-valuation';
import { cn } from '@/lib/utils';

interface LoanValuationNoteProps {
  loan: Loan;
  className?: string;
}

/** Icon with the valuation rule as a tooltip; red-brown when the balance does not fall. */
export function LoanValuationNote({ loan, className }: LoanValuationNoteProps) {
  const { text, mode, warn } = useLoanValuationNote(loan);
  const needsAttention = warn || mode === 'static';
  const Icon = needsAttention ? TriangleAlert : Info;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={text}
          className={cn(
            'inline-flex align-middle rounded-r1 focus-visible:outline-none focus-visible:shadow-focus',
            needsAttention ? 'text-loss' : 'text-ink-4',
            className
          )}
        >
          <Icon className="size-3.5" />
        </button>
      </TooltipTrigger>
      <TooltipContent className="max-w-xs">
        <p>{text}</p>
      </TooltipContent>
    </Tooltip>
  );
}
