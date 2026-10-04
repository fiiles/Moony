/**
 * CategoryFormDialog - create or edit a transaction category (name, icon, color).
 *
 * Shared by Settings → Categories and the "+ New category" entry of the
 * category picker. In edit mode the name field starts from the *displayed*
 * name and `name` is only sent when the user actually changed it, so opening
 * and saving a translated system category never freezes the translation into
 * the database.
 */
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { CategoryIcon } from '@/components/common/CategoryIcon';
import { ICON_MAP } from '@/components/common/category-icons-map';
import { useCategoryName } from '@/hooks/use-categories';
import { useCategoryMutations } from '@/hooks/use-category-mutations';
import { cn } from '@/lib/utils';
import type { TransactionCategory, UpdateTransactionCategory } from '@shared/schema';

const NAME_MAX_CHARS = 50;
const DEFAULT_ICON = 'tag';
const DEFAULT_COLOR = '#6366F1';

/** Distinct, readable on light and dark backgrounds. */
const COLOR_PRESETS = [
  '#EF4444',
  '#F97316',
  '#F59E0B',
  '#84CC16',
  '#22C55E',
  '#14B8A6',
  '#06B6D4',
  '#3B82F6',
  '#6366F1',
  '#8B5CF6',
  '#D946EF',
  '#EC4899',
  '#64748B',
  '#9E9E9E',
];

const ICON_NAMES = Object.keys(ICON_MAP);

interface CategoryFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The category being edited; omit to create a new one. */
  category?: TransactionCategory | null;
  /** Called with the saved category (created or updated). */
  onSaved?: (category: TransactionCategory) => void;
}

export function CategoryFormDialog({
  open,
  onOpenChange,
  category = null,
  onSaved,
}: CategoryFormDialogProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const categoryName = useCategoryName();
  const { createCategory, updateCategory } = useCategoryMutations();

  const isEdit = category !== null;
  const initialName = category ? categoryName(category) : '';
  const [name, setName] = useState(initialName);
  const [icon, setIcon] = useState(category?.icon || DEFAULT_ICON);
  const [color, setColor] = useState(category?.color || DEFAULT_COLOR);

  const trimmedName = name.trim();
  const isPending = createCategory.isPending || updateCategory.isPending;
  const canSave = trimmedName.length > 0 && !isPending;

  const handleSave = async () => {
    if (!canSave) return;
    try {
      let saved: TransactionCategory;
      if (category) {
        // Only send what changed.
        const data: UpdateTransactionCategory = {
          ...(trimmedName !== initialName ? { name: trimmedName } : {}),
          ...(icon !== (category.icon || DEFAULT_ICON) ? { icon } : {}),
          ...(color !== (category.color || DEFAULT_COLOR) ? { color } : {}),
        };
        if (Object.keys(data).length === 0) {
          onOpenChange(false);
          return;
        }
        saved = await updateCategory.mutateAsync({ id: category.id, data });
      } else {
        saved = await createCategory.mutateAsync({ name: trimmedName, icon, color });
      }
      onOpenChange(false);
      onSaved?.(saved);
    } catch {
      // The mutation's onError already showed a translated toast; keep the dialog open.
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {isEdit ? t('categoryManagement.editCategory') : t('categoryManagement.newCategory')}
          </DialogTitle>
          <DialogDescription>
            {isEdit
              ? t('categoryManagement.editDescription')
              : t('categoryManagement.newDescription')}
          </DialogDescription>
        </DialogHeader>

        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault();
            void handleSave();
          }}
        >
          <div className="space-y-2">
            <Label htmlFor="category-name">{t('categoryManagement.name')}</Label>
            <Input
              id="category-name"
              value={name}
              maxLength={NAME_MAX_CHARS}
              placeholder={t('categoryManagement.namePlaceholder')}
              autoFocus
              onChange={(e) => setName(e.target.value)}
            />
          </div>

          <div className="space-y-2">
            <Label>{t('categoryManagement.icon')}</Label>
            <div className="grid grid-cols-9 gap-1" role="radiogroup">
              {ICON_NAMES.map((iconName) => (
                <button
                  key={iconName}
                  type="button"
                  role="radio"
                  aria-checked={icon === iconName}
                  aria-label={iconName}
                  onClick={() => setIcon(iconName)}
                  className={cn(
                    'flex h-8 w-8 items-center justify-center rounded-r1 border transition-colors',
                    icon === iconName
                      ? 'border-dark bg-well-2 text-ink'
                      : 'border-transparent text-ink-3 hover:bg-well'
                  )}
                >
                  <CategoryIcon iconName={iconName} className="h-4 w-4" />
                </button>
              ))}
            </div>
          </div>

          <div className="space-y-2">
            <Label>{t('categoryManagement.color')}</Label>
            <div className="flex flex-wrap items-center gap-2" role="radiogroup">
              {COLOR_PRESETS.map((preset) => (
                <button
                  key={preset}
                  type="button"
                  role="radio"
                  aria-checked={color.toLowerCase() === preset.toLowerCase()}
                  aria-label={preset}
                  onClick={() => setColor(preset)}
                  className="flex h-6 w-6 items-center justify-center rounded-full border border-line"
                  style={{ backgroundColor: preset }}
                >
                  {color.toLowerCase() === preset.toLowerCase() && (
                    <Check className="h-3.5 w-3.5 text-white" />
                  )}
                </button>
              ))}
              <label className="ml-1 flex items-center gap-2 text-xs text-ink-3">
                <input
                  type="color"
                  value={color}
                  aria-label={t('categoryManagement.customColor')}
                  onChange={(e) => setColor(e.target.value.toUpperCase())}
                  className="h-6 w-8 cursor-pointer rounded border border-line bg-transparent p-0"
                />
                {t('categoryManagement.customColor')}
              </label>
            </div>
          </div>

          <div className="flex items-center gap-2 rounded-r1 border bg-well px-3 py-2">
            <span
              className="flex h-7 w-7 items-center justify-center rounded-full"
              style={{ backgroundColor: `${color}26`, color }}
            >
              <CategoryIcon iconName={icon} className="h-4 w-4" />
            </span>
            <span className="truncate text-sm font-medium">
              {trimmedName || t('categoryManagement.namePlaceholder')}
            </span>
          </div>

          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {tc('buttons.cancel')}
            </Button>
            <Button type="submit" disabled={!canSave}>
              {tc('buttons.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export default CategoryFormDialog;
