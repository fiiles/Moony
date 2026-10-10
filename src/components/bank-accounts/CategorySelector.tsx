import { completeAccountIdentifier } from '@/utils/iban-utils';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { translateApiError } from '@/lib/translate-api-error';
/**
 * CategorySelector - Category dropdown with learning and suggestion support
 *
 * Shows a combobox for selecting transaction category with:
 * - Lucide icons mapped from database icon names
 * - A pending MCP suggestion pill (accept on click, decline via X)
 * - Learning on manual selection (toast notification)
 * - "+ New category" at the bottom of the list: creates a category and
 *   selects it for the row straight away
 */

import { useState, useMemo } from 'react';
import {
  ArrowRightLeft,
  Check,
  ChevronsUpDown,
  CircleAlert,
  Flag,
  Sparkles,
  Tag,
  X,
  Brain,
  Plus,
  ListPlus,
} from 'lucide-react';
import { useLocation } from 'wouter';
import { cn } from '@/lib/utils';
import { Button } from '@/components/ui/button';
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Switch } from '@/components/ui/switch';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { toast } from 'sonner';
import { categorizationApi } from '@/lib/tauri-api';
import type { ChangedRow, RecategorizationPreview, TransactionCategory } from '@shared/schema';
import { useTranslation } from 'react-i18next';

import { CategoryIcon } from '@/components/common/CategoryIcon';
import { CategoryFormDialog } from '@/components/categories/CategoryFormDialog';
import { useCategoryName } from '@/hooks/use-categories';
import type { NamedCategory } from '@/utils/category-name';
import { formatBreakdown } from '@/utils/recategorization';
import { editRuleHref } from '@/utils/rules-link';

interface CategorySelectorProps {
  currentCategoryId?: string | null;
  /** Pending MCP suggestion on the row (bank_transactions.suggested_category_id) */
  suggestedCategoryId?: string | null;
  counterpartyName?: string | null;
  counterpartyIban?: string | null;
  bankAccountId?: string;
  isCredit?: boolean;
  counterpartyBankCode?: string | null;
  counterpartyCountry?: string | null;
  categories: TransactionCategory[];
  onCategoryChange: (categoryId: string | null) => void | Promise<void>;
  onAcceptSuggestion?: () => void;
  onDeclineSuggestion?: () => void;
  disabled?: boolean;
  compact?: boolean;
  enableLearning?: boolean;
  /**
   * Description of the transaction row this selector sits in. Names the "Select..." trigger for
   * assistive technology ("Set category for <description>") — in a long list every trigger would
   * otherwise read the same.
   */
  rowLabel?: string;
  /**
   * When set, an uncategorized row (compact mode) shows a "Create a rule" icon button next to the
   * picker — the primary way to categorize a recurring payee for good.
   */
  onCreateRule?: () => void;
  /** The ledger row's id; enables the "other payments" preview in the picker footer. */
  transactionId?: string;
  /**
   * A remembered pick also recategorized these other payments. A list that keeps its own rows
   * (the report drilldown) updates them from here; `onHistoryRestored` reports the undo.
   */
  onHistoryApplied?: (changed: ChangedRow[], categoryId: string) => void;
  onHistoryRestored?: (changed: ChangedRow[]) => void;
}

/** What a pick does besides setting the row's category. */
interface SelectOptions {
  /** Learn from this pick (or, when a custom rule decides, retarget that rule). */
  remember: boolean;
  /** Also give the category to the other payments the rewritten rule covers. */
  applyHistory: boolean;
  /** Display name for a category `categories` does not hold yet (one created a moment ago). */
  categoryName?: string;
}

/**
 * The picker never runs past the viewport: Radix caps it to the room on the chosen side and the
 * list inside scrolls, so the footer (switch, other-payments checkbox) stays reachable.
 */
const PICKER_POPOVER =
  'flex max-h-[var(--radix-popover-content-available-height)] w-[300px] flex-col p-0';

/** Get category display info */
function getCategoryDisplay(categoryId: string | null, categories: TransactionCategory[]) {
  if (!categoryId) return null;
  const category = categories.find((c) => c.id === categoryId);
  if (!category) {
    return null;
  }
  return {
    id: category.id,
    name: category.name,
    isSystem: category.isSystem,
    iconName: category.icon || 'tag',
    color: category.color || '#6b7280',
  };
}

/** Display name of the category with this id; the raw id when it is not in the list. */
function categoryNameById(
  id: string,
  categories: TransactionCategory[],
  categoryName: (category: NamedCategory) => string
): string {
  const category = categories.find((c) => c.id === id);
  return category ? categoryName(category) : id;
}

export function CategorySelector({
  currentCategoryId,
  suggestedCategoryId,
  counterpartyName,
  counterpartyIban,
  categories,
  bankAccountId,
  isCredit,
  counterpartyBankCode,
  counterpartyCountry,
  onCategoryChange,
  onAcceptSuggestion,
  onDeclineSuggestion,
  disabled = false,
  compact = false,
  enableLearning = true,
  rowLabel,
  onCreateRule,
  transactionId,
  onHistoryApplied,
  onHistoryRestored,
}: CategorySelectorProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const categoryName = useCategoryName();
  const [isSaving, setIsSaving] = useState(false);
  const [open, setOpen] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  // Whether the "Remember for future" toggle was off when "+ New category" was clicked.
  const [createSkipLearning, setCreateSkipLearning] = useState(false);
  const [, setLocation] = useLocation();

  const canLearn = enableLearning && !!bankAccountId && isCredit !== undefined;
  // What decides this payee's category today and how many other payments a remembered pick covers.
  const previewKey = ['recategorization-preview', transactionId];
  const loadPreview = () => categorizationApi.previewRecategorization(transactionId as string);
  const preview = useQuery({
    queryKey: previewKey,
    queryFn: loadPreview,
    enabled: open && canLearn && !!transactionId,
    staleTime: 0,
  });

  const currentCategory = useMemo(
    () => getCategoryDisplay(currentCategoryId ?? null, categories),
    [currentCategoryId, categories]
  );

  // A suggestion is shown only while the row has no final category.
  const suggestion = useMemo(
    () => (currentCategory ? null : getCategoryDisplay(suggestedCategoryId ?? null, categories)),
    [currentCategory, suggestedCategoryId, categories]
  );

  const handleDeclineSuggestion = (e: React.MouseEvent) => {
    e.stopPropagation();
    onDeclineSuggestion?.();
  };

  // The change is already saved when this runs, so a failed refresh is reported on its own and
  // never hides the toast (and its undo) of the change that did happen.
  const refreshAfterWrite = async () => {
    try {
      await refreshMoneyFlows(queryClient);
    } catch (e) {
      toast.error(tc('status.error'), { description: translateApiError(e as Error, tc) });
    }
  };

  const historyToast = (title: string, description: string | undefined, changed: ChangedRow[]) => {
    if (changed.length === 0) {
      toast(title, { description });
      return;
    }
    toast(title, {
      description,
      duration: 8000,
      action: {
        label: t('categorization.undoHistory'),
        onClick: () => {
          void (async () => {
            try {
              const restored = await categorizationApi.restoreTransactionCategories(changed);
              onHistoryRestored?.(changed);
              toast(t('categorization.historyRestored', { count: restored }));
            } catch (e) {
              toast.error(tc('status.error'), { description: translateApiError(e as Error, tc) });
              return;
            }
            await refreshAfterWrite();
          })();
        },
      },
    });
  };

  const handleSelect = async (
    categoryId: string,
    options: SelectOptions = { remember: true, applyHistory: false }
  ) => {
    if (categoryId === currentCategoryId) return;

    setIsSaving(true);
    try {
      await onCategoryChange(categoryId);
    } catch {
      setIsSaving(false);
      return;
    }
    setIsSaving(false);
    setOpen(false);

    // One-time categorization (switch off), or learning is disabled for this selector.
    if (!options.remember || !canLearn) return;

    const pickedName =
      options.categoryName ?? categoryNameById(categoryId, categories, categoryName);

    try {
      // What decides this payee decides where the pick goes, so it must not depend on whether the
      // footer's preview happened to load in time: fetch it now when it is missing. Without a
      // transaction id (add-transaction modal, report drilldown) there is no preview and the pick
      // is plainly learned.
      const info = transactionId
        ? (preview.data ??
          (await queryClient.fetchQuery({ queryKey: previewKey, queryFn: loadPreview })))
        : undefined;

      // Own-account transfers categorize themselves and payments without an identity cannot be
      // remembered; a custom rule can still be retargeted either way.
      if (
        info &&
        (info.governing === 'ownAccount' || (!info.learnable && info.governing !== 'custom'))
      ) {
        return;
      }

      if (info?.governing === 'custom' && info.rule) {
        const changed = await categorizationApi.retargetCustomRule(
          info.rule.id,
          categoryId,
          options.applyHistory
        );
        if (changed.length > 0) onHistoryApplied?.(changed, categoryId);
        historyToast(
          changed.length > 0
            ? t('categorization.ruleRetargetedApplied', {
                rule: info.rule.name,
                category: pickedName,
                count: changed.length,
              })
            : t('categorization.ruleRetargeted', { rule: info.rule.name, category: pickedName }),
          undefined,
          changed
        );
        await refreshAfterWrite();
        return;
      }

      // The new rule is limited to this own account, direction and reliable identity.
      const payee =
        counterpartyName && counterpartyName.trim().length > 2 ? counterpartyName.trim() : null;
      const iban =
        counterpartyIban && counterpartyIban.trim().length > 5 ? counterpartyIban.trim() : null;
      // Only learn if we have at least payee or iban
      if (!payee && !iban) return;
      const changed = await categorizationApi.learn(
        {
          payee,
          counterpartyIban: iban,
          categoryId,
          bankAccountId: bankAccountId as string,
          isCredit: isCredit as boolean,
          counterpartyBankCode: counterpartyBankCode ?? null,
          counterpartyCountry: counterpartyCountry ?? null,
        },
        options.applyHistory
      );
      const learnKey = payee || iban || 'unknown';
      if (changed.length > 0) onHistoryApplied?.(changed, categoryId);
      historyToast(
        changed.length > 0
          ? t('categorization.learnedApplied', { count: changed.length })
          : t('categorization.learned'),
        changed.length > 0 ? `"${learnKey}" → ${pickedName}` : `"${learnKey}"`,
        changed
      );
      await refreshAfterWrite();
    } catch (e) {
      // The preview could not be fetched or the write failed: nothing was remembered, the row stays saved.
      toast.error(tc('status.error'), { description: translateApiError(e as Error, tc) });
    }
  };

  const handleRequestCreate = (skipLearning: boolean) => {
    setOpen(false);
    setCreateSkipLearning(skipLearning);
    setCreateOpen(true);
  };

  // What the picker footer needs from the preview, shared by the three render sites.
  const previewProps = {
    preview: preview.data,
    previewLoading: preview.isFetching && !preview.data,
    onEditRule: (ruleId: string) => {
      setOpen(false);
      setLocation(editRuleHref(ruleId));
    },
  };

  const renderSelector = () => {
    // Compact mode for table cells
    if (compact) {
      if (currentCategory) {
        return (
          <Popover open={open} onOpenChange={setOpen}>
            <PopoverTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs font-medium justify-start gap-1.5 hover:bg-well"
                disabled={disabled || isSaving}
              >
                <CategoryIcon iconName={currentCategory.iconName} className="h-3.5 w-3.5" />
                <span className="truncate">{categoryName(currentCategory)}</span>
              </Button>
            </PopoverTrigger>
            <PopoverContent className={PICKER_POPOVER} align="start" collisionPadding={8}>
              <CategoryCommandList
                categories={categories}
                selectedId={currentCategoryId}
                onSelect={handleSelect}
                busy={isSaving}
                scopeLabel={t('readiness.specificLearningScope', {
                  direction: t(isCredit ? 'transaction.credit' : 'transaction.debit'),
                  identity:
                    completeAccountIdentifier(counterpartyIban, null, counterpartyBankCode) ||
                    counterpartyName ||
                    '—',
                })}
                enableLearning={enableLearning}
                onCreate={handleRequestCreate}
                {...previewProps}
              />
            </PopoverContent>
          </Popover>
        );
      }

      // Pending MCP suggestion - amber pill, click accepts, X declines
      if (suggestion) {
        return (
          <div className="flex items-center gap-1">
            <Button
              variant="outline"
              size="sm"
              className="h-7 gap-1.5 border-dashed px-2 text-xs font-medium"
              onClick={() => onAcceptSuggestion?.()}
              disabled={disabled || isSaving}
              title={t('categorization.acceptSuggestion', 'Accept suggestion')}
            >
              <Sparkles className="h-3 w-3 text-ink-3" />
              <CategoryIcon iconName={suggestion.iconName} className="h-3.5 w-3.5 text-ink-2" />
              <span className="truncate text-ink-2">{categoryName(suggestion)}</span>
            </Button>
            {onDeclineSuggestion && (
              <Button
                variant="ghost"
                size="icon-sm"
                className="text-ink-3 hover:text-loss"
                onClick={handleDeclineSuggestion}
                disabled={disabled || isSaving}
                title={t('categorization.declineSuggestion', 'Decline suggestion')}
              >
                <X className="h-3.5 w-3.5" />
              </Button>
            )}
          </div>
        );
      }

      // No category - show simple selector
      return (
        <div className="flex items-center gap-0.5">
          <Popover open={open} onOpenChange={setOpen}>
            <PopoverTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs font-normal text-ink-3 hover:text-ink hover:bg-well gap-1.5"
                disabled={disabled || isSaving}
                aria-label={
                  rowLabel
                    ? t('categorization.setCategoryFor', { description: rowLabel })
                    : undefined
                }
              >
                <Tag className="h-3.5 w-3.5" />
                <span>{t('categorization.selectCategory', 'Select...')}</span>
              </Button>
            </PopoverTrigger>
            <PopoverContent className={PICKER_POPOVER} align="start" collisionPadding={8}>
              <CategoryCommandList
                categories={categories}
                selectedId={currentCategoryId}
                onSelect={handleSelect}
                busy={isSaving}
                scopeLabel={t('readiness.specificLearningScope', {
                  direction: t(isCredit ? 'transaction.credit' : 'transaction.debit'),
                  identity:
                    completeAccountIdentifier(counterpartyIban, null, counterpartyBankCode) ||
                    counterpartyName ||
                    '—',
                })}
                enableLearning={enableLearning}
                onCreate={handleRequestCreate}
                {...previewProps}
              />
            </PopoverContent>
          </Popover>
          {onCreateRule && (
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={
                rowLabel
                  ? t('categorization.createRuleFor', { description: rowLabel })
                  : t('categorization.createRule')
              }
              disabled={disabled || isSaving}
              onClick={onCreateRule}
            >
              <ListPlus />
            </Button>
          )}
        </div>
      );
    }

    // Full-size combobox (non-compact mode)
    return (
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            variant="outline"
            role="combobox"
            aria-expanded={open}
            className="w-[200px] justify-between"
            disabled={disabled || isSaving}
          >
            {currentCategory ? (
              <span className="flex items-center gap-2">
                <CategoryIcon iconName={currentCategory.iconName} className="h-4 w-4" />
                {categoryName(currentCategory)}
              </span>
            ) : suggestion ? (
              <span className="flex items-center gap-2 text-amber-600">
                <Sparkles className="h-4 w-4" />
                {categoryName(suggestion)}
              </span>
            ) : (
              <span className="text-ink-3">
                {t('categorization.selectCategory', 'Select category...')}
              </span>
            )}
            <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 opacity-50" />
          </Button>
        </PopoverTrigger>
        <PopoverContent className={PICKER_POPOVER} collisionPadding={8}>
          <CategoryCommandList
            categories={categories}
            selectedId={currentCategoryId}
            onSelect={handleSelect}
            busy={isSaving}
            scopeLabel={t('readiness.specificLearningScope', {
              direction: t(isCredit ? 'transaction.credit' : 'transaction.debit'),
              identity:
                completeAccountIdentifier(counterpartyIban, null, counterpartyBankCode) ||
                counterpartyName ||
                '—',
            })}
            enableLearning={enableLearning}
            onCreate={handleRequestCreate}
            {...previewProps}
          />
        </PopoverContent>
      </Popover>
    );
  };

  return (
    <>
      {renderSelector()}
      {createOpen && (
        <CategoryFormDialog
          open
          onOpenChange={(next) => {
            if (!next) setCreateOpen(false);
          }}
          onSaved={(created) =>
            void handleSelect(created.id, {
              remember: !createSkipLearning,
              applyHistory: false,
              // `categories` does not hold the new category yet.
              categoryName: categoryName(created),
            })
          }
        />
      )}
    </>
  );
}

/** Category command list used in popovers */
function CategoryCommandList({
  categories,
  selectedId,
  onSelect,
  enableLearning = true,
  scopeLabel,
  busy = false,
  onCreate,
  preview,
  previewLoading = false,
  onEditRule,
}: {
  categories: TransactionCategory[];
  selectedId?: string | null;
  onSelect: (id: string, options: SelectOptions) => void;
  enableLearning?: boolean;
  scopeLabel: string;
  busy?: boolean;
  /** Called with `skipLearning` when "+ New category" is clicked. */
  onCreate: (skipLearning: boolean) => void;
  /** What decides this payee's category today and what a remembered pick would cover. */
  preview?: RecategorizationPreview;
  previewLoading?: boolean;
  /** Opens a custom rule's editor. */
  onEditRule?: (ruleId: string) => void;
}) {
  const { t } = useTranslation('bank_accounts');
  const categoryName = useCategoryName();
  const [rememberForFuture, setRememberForFuture] = useState(true);
  const [applyHistory, setApplyHistory] = useState(true);

  // The checkbox is only on screen when the preview found other payments; a pick made before that
  // never recategorizes history the user has not seen.
  const historyOptions: SelectOptions = {
    remember: rememberForFuture,
    applyHistory: applyHistory && (preview?.matches ?? 0) > 0,
  };
  const governingRule = preview?.governing === 'custom' ? preview.rule : null;

  // Sort categories by sortOrder
  const sortedCategories = useMemo(() => {
    return [...categories]
      .filter((c) => !c.parentId)
      .sort((a, b) => (a.sortOrder || 0) - (b.sortOrder || 0));
  }, [categories]);

  return (
    <Command className="min-h-0">
      <CommandInput placeholder={t('categorization.searchCategory', 'Search...')} />
      <CommandList className="max-h-[300px] min-h-0 flex-1">
        <CommandEmpty>{t('categorization.noCategory', 'No category found')}</CommandEmpty>
        <CommandGroup>
          {sortedCategories.map((category) => (
            <CommandItem
              key={category.id}
              disabled={busy}
              value={categoryName(category)}
              onSelect={() => onSelect(category.id, historyOptions)}
              className="flex items-center gap-2 py-2"
            >
              <Check
                className={cn(
                  'h-4 w-4 shrink-0',
                  selectedId === category.id ? 'opacity-100' : 'opacity-0'
                )}
              />
              <CategoryIcon
                iconName={category.icon || 'tag'}
                className="h-4 w-4 shrink-0 text-ink-3"
              />
              <span className="truncate">{categoryName(category)}</span>
            </CommandItem>
          ))}
        </CommandGroup>
      </CommandList>
      {/* Always visible, outside the scrolling list */}
      <button
        type="button"
        disabled={busy}
        onClick={() => onCreate(!rememberForFuture)}
        className="flex w-full shrink-0 items-center gap-2 border-t px-3 py-2 text-sm text-ink-3 transition-colors hover:bg-well hover:text-ink"
      >
        <Plus className="h-4 w-4 shrink-0" />
        {t('categoryManagement.newCategory')}
      </button>
      {/* Learning footer - only if learning is enabled */}
      {enableLearning && (
        <div className="grid shrink-0 gap-2 border-t px-3 py-2.5">
          {preview?.governing === 'ownAccount' ? (
            <p className="m-0 flex items-start gap-2 text-xs text-ink-3">
              <ArrowRightLeft className="mt-px h-3.5 w-3.5 shrink-0 text-ink-4" />
              {t('categorization.ownAccountNote')}
            </p>
          ) : preview && !preview.learnable && preview.governing !== 'custom' ? (
            <p className="m-0 flex items-start gap-2 text-xs text-ink-3">
              <CircleAlert className="mt-px h-3.5 w-3.5 shrink-0 text-ink-4" />
              {t('categorization.notLearnable')}
            </p>
          ) : (
            <>
              {governingRule && (
                <div className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5 rounded-md bg-well px-2.5 py-2 text-xs font-medium text-ink-2">
                  <Flag className="row-span-2 mt-px h-3.5 w-3.5 text-ink-3" />
                  <span>
                    {t('categorization.governedByRule', {
                      rule: governingRule.name,
                      category: categoryNameById(
                        governingRule.categoryId,
                        categories,
                        categoryName
                      ),
                    })}
                  </span>
                  {onEditRule && (
                    <button
                      type="button"
                      className="justify-self-start text-micro font-650 text-ink-2 hover:underline"
                      onClick={() => onEditRule(governingRule.id)}
                    >
                      {t('categorization.editRule')}
                    </button>
                  )}
                </div>
              )}
              <div className="flex items-start justify-between gap-2">
                <div className="flex items-start gap-2">
                  <Brain className="mt-px h-3.5 w-3.5 shrink-0 text-ink-3" />
                  <Label
                    htmlFor="remember-toggle"
                    className="cursor-pointer flex-col items-start justify-start gap-0.5 text-xs leading-snug text-ink-3"
                  >
                    <span>
                      {governingRule
                        ? t('categorization.retargetRule')
                        : t('categorization.rememberForFuture')}
                    </span>
                    <small className="block font-400">
                      {governingRule ? t('categorization.retargetRuleScope') : scopeLabel}
                    </small>
                  </Label>
                </div>
                <Switch
                  id="remember-toggle"
                  checked={rememberForFuture}
                  onCheckedChange={setRememberForFuture}
                  className="scale-75"
                />
              </div>
              {rememberForFuture && previewLoading && (
                <p className="m-0 text-micro font-500 text-ink-4" aria-live="polite">
                  {t('categorization.historyChecking')}
                </p>
              )}
              {rememberForFuture && preview && preview.matches > 0 && (
                <label className="flex cursor-pointer items-start gap-2 text-xs text-ink-2">
                  <Checkbox
                    className="mt-px"
                    checked={applyHistory}
                    onCheckedChange={(v) => setApplyHistory(v === true)}
                  />
                  <span>
                    <b className="block font-600">
                      {t('categorization.applyHistory', { count: preview.matches })}
                    </b>
                    <small className="block text-micro font-500 text-ink-4">
                      {formatBreakdown(
                        preview.byCategory,
                        (id) => categoryNameById(id, categories, categoryName),
                        {
                          uncategorized: t('categorization.uncategorizedCount'),
                          more: (n) => t('categorization.moreCategories', { count: n }),
                        }
                      )}
                    </small>
                  </span>
                </label>
              )}
            </>
          )}
        </div>
      )}
    </Command>
  );
}

export default CategorySelector;
