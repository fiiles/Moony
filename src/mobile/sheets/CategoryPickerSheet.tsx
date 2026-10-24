import { useId, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, Sparkles, X } from 'lucide-react';
import type { BankTransaction, ChangedRow, TransactionCategory } from '@shared/schema';
import { CategoryIconChip } from '@/components/common/CategoryIcon';
import { CategoryFormDialog } from '@/components/categories/CategoryFormDialog';
import { Button } from '@/components/ui/button';
import { useCategoryName } from '@/hooks/use-categories';
import { cn } from '@/lib/utils';
import { formatBreakdown } from '@/utils/recategorization';
import { completeAccountIdentifier } from '@/utils/iban-utils';
import { NeutralChip, PickGroup, PickRow } from '@/mobile/components/PickRow';
import { SearchField } from '@/mobile/components/SearchField';
import { Sheet } from '@/mobile/components/Sheet';
import { SwitchRow } from '@/mobile/components/SwitchRow';
import { useCategorize } from '@/mobile/hooks/use-categorize';
import { useLedgerMutations } from '@/mobile/hooks/use-ledger-mutations';
import { useRetained } from '@/mobile/hooks/use-retained';
import { filterCategories } from '@/mobile/lib/category-pick';

interface CategoryPickerSheetProps {
  /** The row to categorize; `null` while the sheet is closed (it keeps the last one while it slides away). */
  tx: BankTransaction | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  categories: readonly TransactionCategory[];
  /** The categories of the account's newest categorized rows, newest first (`recentCategoryIds` of its whole history). */
  recentIds: readonly string[];
  /** Says what the sheet has changed on the row, so that the copy the caller holds follows. */
  onChange?: (patch: Partial<BankTransaction>) => void;
  /**
   * Hands the other payments a remembered pick took along to the caller (`PickOptions.onApplied`).
   * A caller that takes them shows them itself — the categorize queue's expansion, with its own
   * undo — so the sheet then leaves out the history toast of a pick that took other payments
   * along (`quiet`). The ledger passes nothing.
   */
  onApplied?: (changed: ChangedRow[]) => void;
}

/**
 * The category picker (spec §8.4, README §13): a half sheet with the search, the MCP-suggested
 * category as a pill to accept or decline, the recently used categories, then all of them, and
 * "Nová kategorie". A tap on a category saves it at once and closes the sheet. Under the list,
 * pinned to the bottom while the search is empty, are the two learning options the desktop's
 * picker has — remember the pick for this account and direction, and also categorize the other
 * payments it covers — which the tap takes as they stand (`useCategorize`).
 */
export function CategoryPickerSheet({
  tx,
  open,
  onOpenChange,
  categories,
  recentIds,
  onChange,
  onApplied,
}: CategoryPickerSheetProps) {
  const { t: tc } = useTranslation('common');
  const { t } = useTranslation('mobile');
  const shown = useRetained(tx);
  const close = () => onOpenChange(false);

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('picker.title')}
      footer={
        <Button
          variant="ghost"
          className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
          onClick={close}
        >
          {tc('buttons.cancel')}
        </Button>
      }
    >
      {/* Its own component: the search and the two switches start over every time the sheet opens */}
      {shown && (
        <PickerBody
          tx={shown}
          categories={categories}
          recentIds={recentIds}
          onClose={close}
          onChange={onChange}
          onApplied={onApplied}
        />
      )}
    </Sheet>
  );
}

function PickerBody({
  tx,
  categories,
  recentIds,
  onClose,
  onChange,
  onApplied,
}: {
  tx: BankTransaction;
  categories: readonly TransactionCategory[];
  recentIds: readonly string[];
  onClose: () => void;
  onChange?: (patch: Partial<BankTransaction>) => void;
  onApplied?: (changed: ChangedRow[]) => void;
}) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const categoryName = useCategoryName();
  const { acceptSuggestion, declineSuggestion } = useLedgerMutations();
  const { preview, previewLoading, pick, busy } = useCategorize(tx, categories);
  const rememberId = useId();
  const applyId = useId();

  const [query, setQuery] = useState('');
  const [remember, setRemember] = useState(true);
  const [applyHistory, setApplyHistory] = useState(true);
  const [createOpen, setCreateOpen] = useState(false);

  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);
  const nameOfId = (id: string) => {
    const category = categoryById.get(id);
    return category ? categoryName(category) : id;
  };
  const searching = query.trim() !== '';
  const all = filterCategories(categories, query, categoryName);
  const recent = recentIds.flatMap((id) => categoryById.get(id) ?? []);
  const suggestion = tx.categoryId ? undefined : categoryById.get(tx.suggestedCategoryId ?? '');

  // What decides this payee today: an own-account transfer categorizes itself, a payment
  // without an identity cannot be remembered, a custom rule is re-pointed instead of learned.
  const governingRule = preview?.governing === 'custom' ? preview.rule : null;
  const blocked =
    preview !== undefined &&
    (preview.governing === 'ownAccount' || (!preview.learnable && preview.governing !== 'custom'));
  const remembering = remember && !blocked;
  const matches = preview?.matches ?? 0;
  const scope = tb('readiness.specificLearningScope', {
    direction: tb(tx.type === 'credit' ? 'transaction.credit' : 'transaction.debit'),
    identity:
      completeAccountIdentifier(tx.counterpartyIban, null, tx.counterpartyBankCode) ||
      tx.counterpartyName ||
      '—',
  });

  /** Saves the category and, once it is saved, closes the sheet; a failed save leaves it open. */
  const choose = async (id: string, options: { applyHistory: boolean; categoryName?: string }) => {
    if (busy) return;
    const saved = await pick(id, {
      remember: remembering,
      ...options,
      onApplied,
      quiet: onApplied !== undefined,
    });
    if (!saved) return;
    onChange?.({ categoryId: id });
    onClose();
  };
  const choosePicked = (id: string) =>
    void choose(id, { applyHistory: applyHistory && matches > 0 });

  const accept = () =>
    acceptSuggestion.mutate(tx.id, {
      onSuccess: () => {
        onChange?.({ categoryId: tx.suggestedCategoryId, suggestedCategoryId: null });
        onClose();
      },
    });
  const decline = () =>
    declineSuggestion.mutate(tx.id, {
      onSuccess: () => onChange?.({ suggestedCategoryId: null }),
    });

  const row = (category: TransactionCategory) => (
    <PickRow
      key={category.id}
      icon={<CategoryIconChip category={category} />}
      label={categoryName(category)}
      checked={tx.categoryId === category.id}
      onSelect={() => choosePicked(category.id)}
    />
  );

  let rememberSub: string;
  if (preview?.governing === 'ownAccount') rememberSub = tb('categorization.ownAccountNote');
  else if (blocked) rememberSub = tb('categorization.notLearnable');
  else rememberSub = governingRule ? tb('categorization.retargetRuleScope') : scope;

  return (
    <>
      <SearchField
        value={query}
        onChange={setQuery}
        placeholder={tb('categorization.searchCategory')}
        clearLabel={tc('a11y.clearSearch')}
      />
      {suggestion && (
        <PickGroup title={t('picker.suggested')} role="group">
          <div className="flex items-center gap-1 py-1">
            <button
              type="button"
              onClick={accept}
              aria-label={`${tb('categorization.acceptSuggestion')}: ${categoryName(suggestion)}`}
              className="relative inline-flex min-h-9 min-w-0 items-center gap-2 rounded-[10px] border border-dashed border-line-strong bg-paper py-1 pl-2.5 pr-3 text-m-sub font-semibold text-ink-2 [-webkit-touch-callout:none] after:absolute after:inset-x-0 after:-inset-y-1 after:content-[''] active:bg-well focus-visible:outline-none focus-visible:shadow-focus"
            >
              <Sparkles aria-hidden className="size-3.5 flex-none text-ink-3" strokeWidth={1.75} />
              <CategoryIconChip category={suggestion} className="size-6 [&_svg]:size-3.5" />
              <span className="truncate">{categoryName(suggestion)}</span>
            </button>
            <button
              type="button"
              onClick={decline}
              aria-label={t('picker.decline')}
              className="inline-flex size-11 flex-none items-center justify-center rounded-[12px] text-ink-3 [-webkit-touch-callout:none] active:text-loss focus-visible:outline-none focus-visible:shadow-focus"
            >
              <X aria-hidden className="size-4" strokeWidth={1.75} />
            </button>
          </div>
        </PickGroup>
      )}
      {!searching && recent.length > 0 && (
        <PickGroup title={t('picker.recent')}>{recent.map(row)}</PickGroup>
      )}
      {all.length > 0 ? (
        <PickGroup title={t('picker.all')}>{all.map(row)}</PickGroup>
      ) : (
        <p className="mx-1 my-3 text-m-body text-ink-3">{t('picker.empty')}</p>
      )}
      <PickRow
        icon={<NeutralChip icon={<Plus />} />}
        label={tb('categoryManagement.newCategory')}
        onSelect={() => setCreateOpen(true)}
      />

      {/* Under the list, pinned to the sheet's bottom so that the switches the tap takes as they stand
          are in view — until the search narrows the list: with the keyboard up the list needs the room */}
      <div
        className={cn(
          '-mx-4 -mb-3.5 mt-2 border-t border-line bg-paper px-4 pb-1.5 pt-1',
          !searching && 'sticky bottom-0'
        )}
      >
        {governingRule && (
          <p className="mx-1 mb-0 mt-1.5 text-m-sub text-ink-3">
            {tb('categorization.governedByRule', {
              rule: governingRule.name,
              category: nameOfId(governingRule.categoryId),
            })}
          </p>
        )}
        <SwitchRow
          id={rememberId}
          label={
            governingRule
              ? tb('categorization.retargetRule')
              : tb('categorization.rememberForFuture')
          }
          sub={rememberSub}
          checked={remembering}
          onCheckedChange={setRemember}
          disabled={blocked}
        />
        {remembering && previewLoading && (
          <p className="mx-1 my-1 text-m-caption text-ink-4" aria-live="polite">
            {tb('categorization.historyChecking')}
          </p>
        )}
        {remembering && matches > 0 && preview && (
          <SwitchRow
            id={applyId}
            label={tb('categorization.applyHistory', { count: matches })}
            sub={formatBreakdown(preview.byCategory, nameOfId, {
              uncategorized: tb('categorization.uncategorizedCount'),
              more: (count) => tb('categorization.moreCategories', { count }),
            })}
            checked={applyHistory}
            onCheckedChange={setApplyHistory}
          />
        )}
      </div>

      {createOpen && (
        <CategoryFormDialog
          open
          onOpenChange={(next) => {
            if (!next) setCreateOpen(false);
          }}
          onSaved={(created) =>
            // `categories` does not hold the new category yet
            void choose(created.id, { applyHistory: false, categoryName: categoryName(created) })
          }
        />
      )}
    </>
  );
}
