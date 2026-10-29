import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { FLOW_KINDS, type FlowKind } from '@shared/schema';
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
import { DEFAULT_COLOR, IconColorPicker } from '@/components/common/IconColorPicker';
import { useCategoryName } from '@/hooks/use-categories';
import { useCategoryMutations } from '@/hooks/use-category-mutations';
import type { TransactionCategory, UpdateTransactionCategory } from '@shared/schema';

const NAME_MAX_CHARS = 50;
const DEFAULT_ICON = 'tag';

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
  const [kind, setKind] = useState<FlowKind>(category?.kind ?? 'expense');
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
          ...(kind !== category.kind ? { kind } : {}),
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
        saved = await createCategory.mutateAsync({ name: trimmedName, icon, color, kind });
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
            <Label>{t('readiness.categoryRole')}</Label>
            <Select value={kind} onValueChange={(value) => setKind(value as FlowKind)}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {FLOW_KINDS.map((value) => (
                  <SelectItem key={value} value={value}>
                    {t(`readiness.flow.${value}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-micro text-ink-4">{t('readiness.roleImpact')}</p>
          </div>
          <IconColorPicker
            icon={icon}
            color={color}
            onIconChange={setIcon}
            onColorChange={setColor}
            previewLabel={trimmedName || t('categoryManagement.namePlaceholder')}
          />

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
