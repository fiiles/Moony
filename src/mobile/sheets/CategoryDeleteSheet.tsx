import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronRight } from 'lucide-react';
import type { TransactionCategory } from '@shared/schema';
import { CategoryIconChip } from '@/components/common/CategoryIcon';
import { Button } from '@/components/ui/button';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { useCategoryMutations } from '@/hooks/use-category-mutations';
import { PickGroup, PickRow } from '@/mobile/components/PickRow';
import { Sheet } from '@/mobile/components/Sheet';
import { canConfirmDelete, reassignTargets, type CategoryRow } from '@/mobile/lib/category-rows';

interface CategoryDeleteSheetProps {
  /** The category to delete; `null` closes the sheet (it keeps the last one while it slides away). */
  row: CategoryRow | null;
  /** The usage worded for the sentence ("Transakce: 3, pravidla: 1, …"); kept in full, as the desktop words it. */
  usageText: string;
  onClose: () => void;
}

/**
 * The confirmation before a category goes (the phone counterpart of the desktop's delete dialog):
 * the question, what the category holds, and — for a category in use — a "Přesunout vše do" row
 * that opens the list of categories to move it to (its own kind first, never itself). The solid
 * button waits for that pick; an unused category goes at once.
 */
export function CategoryDeleteSheet({ row, usageText, onClose }: CategoryDeleteSheetProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const categoryName = useCategoryName();
  const { categories } = useCategories();
  const { deleteCategory } = useCategoryMutations();
  const [shown, setShown] = useState(row);
  const [reassignTo, setReassignTo] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);
  const [wasOpen, setWasOpen] = useState(row !== null);
  // Every opening starts clean: a pick made before "Zrušit" must not survive into the next one
  if (row !== null && (row !== shown || !wasOpen)) {
    setShown(row);
    setReassignTo(null);
    setPicking(false);
  }
  if (wasOpen !== (row !== null)) setWasOpen(row !== null);

  const targets = shown ? reassignTargets(categories, shown.category) : [];
  const target: TransactionCategory | undefined = targets.find((c) => c.id === reassignTo);
  const confirmable = shown !== null && canConfirmDelete(shown, reassignTo);

  const confirm = async () => {
    if (!shown || !confirmable || deleteCategory.isPending) return;
    try {
      await deleteCategory.mutateAsync({
        id: shown.category.id,
        reassignTo: shown.inUse ? reassignTo : null,
      });
      onClose();
    } catch {
      // onError already showed a translated toast; keep the sheet open.
    }
  };

  return (
    <>
      <Sheet
        open={row !== null}
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
        title={t('categoryManagement.deleteTitle', {
          name: shown ? categoryName(shown.category) : '',
        })}
        description={
          shown?.inUse
            ? t('categoryManagement.deleteInUseDescription', { usage: usageText })
            : t('categoryManagement.deleteUnusedDescription')
        }
        footer={
          <>
            <Button
              variant="ghost"
              className="h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] px-[18px] text-m-row"
              onClick={onClose}
            >
              {tc('buttons.cancel')}
            </Button>
            <Button
              variant="destructive"
              className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
              loading={deleteCategory.isPending}
              disabled={!confirmable}
              onClick={() => void confirm()}
            >
              {t('categoryManagement.deleteConfirm')}
            </Button>
          </>
        }
      >
        {shown?.inUse ? (
          <button
            type="button"
            onClick={() => setPicking(true)}
            className="flex min-h-11 w-full items-center gap-3 rounded-[12px] border border-line px-3 py-2 text-left text-m-body text-ink active:bg-well focus-visible:outline-none focus-visible:shadow-focus"
          >
            <span className="min-w-0 flex-1">
              <span className="block text-m-sub font-semibold text-ink-3">
                {t('categoryManagement.moveTo')}
              </span>
              <span className="block break-words">
                {target ? categoryName(target) : t('categoryManagement.moveToPlaceholder')}
              </span>
            </span>
            <ChevronRight aria-hidden className="size-4 flex-none text-ink-5" strokeWidth={1.75} />
          </button>
        ) : null}
      </Sheet>
      <Sheet
        open={picking}
        onOpenChange={setPicking}
        title={t('categoryManagement.moveTo')}
        full
        dismiss="cancel"
      >
        <PickGroup title={t('categoryManagement.moveToPlaceholder')}>
          {targets.map((c) => (
            <PickRow
              key={c.id}
              icon={<CategoryIconChip category={c} />}
              label={categoryName(c)}
              checked={reassignTo === c.id}
              onSelect={() => {
                setReassignTo(c.id);
                setPicking(false);
              }}
            />
          ))}
        </PickGroup>
      </Sheet>
    </>
  );
}
