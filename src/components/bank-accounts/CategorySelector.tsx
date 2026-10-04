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
import { Check, ChevronsUpDown, Sparkles, Tag, X, Brain, Plus, ListPlus } from 'lucide-react';
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
import { Label } from '@/components/ui/label';
import { toast } from 'sonner';
import { categorizationApi } from '@/lib/tauri-api';
import type { TransactionCategory } from '@shared/schema';
import { useTranslation } from 'react-i18next';

import { CategoryIcon } from '@/components/common/CategoryIcon';
import { CategoryFormDialog } from '@/components/categories/CategoryFormDialog';
import { useCategoryName } from '@/hooks/use-categories';

interface CategorySelectorProps {
  currentCategoryId?: string | null;
  /** Pending MCP suggestion on the row (bank_transactions.suggested_category_id) */
  suggestedCategoryId?: string | null;
  counterpartyName?: string | null;
  counterpartyIban?: string | null;
  categories: TransactionCategory[];
  onCategoryChange: (categoryId: string | null) => void;
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
}

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

export function CategorySelector({
  currentCategoryId,
  suggestedCategoryId,
  counterpartyName,
  counterpartyIban,
  categories,
  onCategoryChange,
  onAcceptSuggestion,
  onDeclineSuggestion,
  disabled = false,
  compact = false,
  enableLearning = true,
  rowLabel,
  onCreateRule,
}: CategorySelectorProps) {
  const { t } = useTranslation('bank_accounts');
  const categoryName = useCategoryName();
  const [open, setOpen] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  // Whether the "Remember for future" toggle was off when "+ New category" was clicked.
  const [createSkipLearning, setCreateSkipLearning] = useState(false);

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

  const handleSelect = async (categoryId: string, skipLearning: boolean = false) => {
    setOpen(false);
    if (categoryId === currentCategoryId) return;

    onCategoryChange(categoryId);

    // Skip learning if explicitly requested (one-time categorization)
    // OR if learning is disabled for this selector
    if (skipLearning || !enableLearning) {
      return;
    }

    // Learn from counterparty details with hierarchical matching:
    // - payee + iban = iban default
    // - payee only (null iban) = payee default
    const payee =
      counterpartyName && counterpartyName.trim().length > 2 ? counterpartyName.trim() : null;
    const iban =
      counterpartyIban && counterpartyIban.trim().length > 5 ? counterpartyIban.trim() : null;

    // Only learn if we have at least payee or iban
    if (payee || iban) {
      try {
        await categorizationApi.learn(payee, iban, categoryId);
        const learnKey = payee || iban || 'unknown';
        toast(t('categorization.learned'), { description: `"${learnKey}"` });
      } catch (e) {
        console.error('Failed to learn categorization:', e);
      }
    }
  };

  const handleRequestCreate = (skipLearning: boolean) => {
    setOpen(false);
    setCreateSkipLearning(skipLearning);
    setCreateOpen(true);
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
                disabled={disabled}
              >
                <CategoryIcon iconName={currentCategory.iconName} className="h-3.5 w-3.5" />
                <span className="truncate">{categoryName(currentCategory)}</span>
              </Button>
            </PopoverTrigger>
            <PopoverContent className="w-[220px] p-0" align="start">
              <CategoryCommandList
                categories={categories}
                selectedId={currentCategoryId}
                onSelect={handleSelect}
                enableLearning={enableLearning}
                onCreate={handleRequestCreate}
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
              disabled={disabled}
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
                disabled={disabled}
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
                disabled={disabled}
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
            <PopoverContent className="w-[220px] p-0" align="start">
              <CategoryCommandList
                categories={categories}
                selectedId={currentCategoryId}
                onSelect={handleSelect}
                enableLearning={enableLearning}
                onCreate={handleRequestCreate}
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
              disabled={disabled}
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
            disabled={disabled}
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
        <PopoverContent className="w-[220px] p-0">
          <CategoryCommandList
            categories={categories}
            selectedId={currentCategoryId}
            onSelect={handleSelect}
            enableLearning={enableLearning}
            onCreate={handleRequestCreate}
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
          onSaved={(created) => void handleSelect(created.id, createSkipLearning)}
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
  onCreate,
}: {
  categories: TransactionCategory[];
  selectedId?: string | null;
  onSelect: (id: string, skipLearning: boolean) => void;
  enableLearning?: boolean;
  /** Called with `skipLearning` when "+ New category" is clicked. */
  onCreate: (skipLearning: boolean) => void;
}) {
  const { t } = useTranslation('bank_accounts');
  const categoryName = useCategoryName();
  const [rememberForFuture, setRememberForFuture] = useState(true);

  // Sort categories by sortOrder
  const sortedCategories = useMemo(() => {
    return [...categories]
      .filter((c) => !c.parentId)
      .sort((a, b) => (a.sortOrder || 0) - (b.sortOrder || 0));
  }, [categories]);

  return (
    <Command>
      <CommandInput placeholder={t('categorization.searchCategory', 'Search...')} />
      <CommandList className="max-h-[300px]">
        <CommandEmpty>{t('categorization.noCategory', 'No category found')}</CommandEmpty>
        <CommandGroup>
          {sortedCategories.map((category) => (
            <CommandItem
              key={category.id}
              value={categoryName(category)}
              onSelect={() => onSelect(category.id, !rememberForFuture)}
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
        onClick={() => onCreate(!rememberForFuture)}
        className="flex w-full items-center gap-2 border-t px-3 py-2 text-sm text-ink-3 transition-colors hover:bg-well hover:text-ink"
      >
        <Plus className="h-4 w-4 shrink-0" />
        {t('categoryManagement.newCategory')}
      </button>
      {/* Toggle for learning preference - only if learning is enabled */}
      {enableLearning && (
        <div className="border-t px-3 py-2.5">
          <div className="flex items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              <Brain className="h-3.5 w-3.5 text-ink-3" />
              <Label htmlFor="remember-toggle" className="text-xs text-ink-3 cursor-pointer">
                {t('categorization.rememberForFuture', 'Remember for future')}
              </Label>
            </div>
            <Switch
              id="remember-toggle"
              checked={rememberForFuture}
              onCheckedChange={setRememberForFuture}
              className="scale-75"
            />
          </div>
        </div>
      )}
    </Command>
  );
}

export default CategorySelector;
