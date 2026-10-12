import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { barLayout, type ContributionItem } from './contribution-model';
import { ContributionRow, ContributionTotalRow, DIALOG_GRID } from './ContributionRow';

interface ContributionOtherDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** What the card folded into "Ostatní", best contribution first. */
  items: readonly ContributionItem[];
  /** What they add up to, percentage points. */
  total: number;
  /** The one sentence under the title. */
  description: string;
}

/**
 * "Ostatní" of the contribution card opened: every position (or part of the group) that did not
 * fit in the card, listed the way the card lists its rows, with the bars scaled to these items
 * and the sum under them. The list scrolls inside the dialog when it is long.
 */
export function ContributionOtherDialog({
  open,
  onOpenChange,
  items,
  total,
  description,
}: ContributionOtherDialogProps) {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const layout = useMemo(() => barLayout(items.map((item) => item.points)), [items]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('stocksAnalysis.contribution.otherTitle')}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <div className="-mx-[27px] -mb-4 -mt-4 flex flex-col">
          {/* The first row sits under the head's hairline: no second rule above it */}
          <ul className="m-0 max-h-[60vh] list-none overflow-y-auto p-0 [&>li:first-child]:border-t-0">
            {items.map((item, i) => (
              <ContributionRow
                key={item.id}
                row={item}
                zero={layout.zero}
                bar={layout.bars[i]}
                grid={DIALOG_GRID}
              />
            ))}
          </ul>
          <ul className="m-0 list-none p-0">
            <ContributionTotalRow
              label={t('stocksAnalysis.contribution.otherTotal')}
              points={total}
              grid={DIALOG_GRID}
              className="bg-paper"
            />
          </ul>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
