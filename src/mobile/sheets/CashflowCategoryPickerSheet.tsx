import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Loader2 } from 'lucide-react';
import type { FlowKind, ReportTransaction, TransactionCategory } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useCategoryName } from '@/hooks/use-categories';
import { CategoryChip } from '@/mobile/components/cashflow/CategoryChip';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { Sheet } from '@/mobile/components/Sheet';

/** The groups of the list, in order. */
const KINDS: readonly FlowKind[] = ['expense', 'income', 'transfer'];

interface CashflowCategoryPickerSheetProps {
  /** The payment being recategorized; `tx.categoryId` is the category it has now. */
  tx: ReportTransaction | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  categories: readonly TransactionCategory[];
  /** Gives the payment its new category; a rejection means it did not take (the caller says why). */
  onPicked: (txId: string, categoryId: string) => Promise<void>;
}

/**
 * "Kategorie" (spec §8.7): the thin picker of the category drilldown — every category as a 52 pt
 * row (chip and name), grouped by výdaj, příjem and převod, the payment's current one ticked. No
 * search, no recents, no learning: a tap gives the payment that category (a tap on its current
 * one just closes the sheet). The tapped row shows a spinner while `onPicked` runs, the list
 * answers no other tap, and the sheet closes when the pick has taken; when it has not, it stays
 * open for another try. The tick is named ("Aktuální kategorie") for a screen reader. It keeps the
 * last payment while it slides away.
 */
export function CashflowCategoryPickerSheet({
  tx,
  open,
  onOpenChange,
  categories,
  onPicked,
}: CashflowCategoryPickerSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const categoryName = useCategoryName();
  // `tx` goes null as the sheet closes; the sheet slides away over the payment it showed
  const [shown, setShown] = useState(tx);
  if (tx !== null && tx !== shown) setShown(tx);
  // The category being applied, and to which payment: a sheet reopened for another one shows none
  const [pending, setPending] = useState<{ txId: string; categoryId: string } | null>(null);
  const applying = pending !== null && pending.txId === shown?.id ? pending : null;
  // Which opening a pick belongs to: one that outlives its opening must not close the next
  const opening = useRef(0);
  useEffect(() => {
    if (open) opening.current += 1;
  }, [open]);

  const pick = async (categoryId: string) => {
    if (shown === null || applying !== null) return;
    // Already the payment's category: nothing to write
    if (categoryId === shown.categoryId) {
      onOpenChange(false);
      return;
    }
    const mine = opening.current;
    const attempt = { txId: shown.id, categoryId };
    setPending(attempt);
    try {
      await onPicked(shown.id, categoryId);
      if (opening.current === mine) onOpenChange(false);
    } catch {
      // The caller has told the user what went wrong; the sheet stays for another try.
    } finally {
      // Only this attempt's own spinner: a later pick for another payment keeps its own
      setPending((current) => (current === attempt ? null : current));
    }
  };

  const groups = KINDS.map((kind) => ({
    kind,
    list: categories.filter((category) => category.kind === kind),
  })).filter((group) => group.list.length > 0);

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('cashflow.picker.title')}
      full
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
      <div aria-busy={applying !== null}>
        {groups.map(({ kind, list }) => (
          <section key={kind}>
            <SectionHead title={tb(`readiness.flow.${kind}`)} />
            <EntityRows>
              {list.map((category) => (
                <EntityRow
                  key={category.id}
                  chip={<CategoryChip category={category} />}
                  title={categoryName(category)}
                  value={
                    applying?.categoryId === category.id ? (
                      <Loader2
                        aria-hidden
                        strokeWidth={1.75}
                        className="inline size-4 animate-spin align-[-3px] motion-reduce:animate-none"
                      />
                    ) : category.id === shown?.categoryId ? (
                      <Check
                        role="img"
                        aria-label={t('cashflow.picker.current')}
                        strokeWidth={1.75}
                        className="inline size-4 align-[-3px]"
                      />
                    ) : undefined
                  }
                  onClick={() => void pick(category.id)}
                />
              ))}
            </EntityRows>
          </section>
        ))}
      </div>
    </Sheet>
  );
}
