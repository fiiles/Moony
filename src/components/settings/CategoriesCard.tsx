import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Pencil, Plus, Trash2 } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { CategoryIcon } from '@/components/common/CategoryIcon';
import { CategoryFormDialog } from '@/components/categories/CategoryFormDialog';
import { SettingsCard } from '@/components/settings/SettingsCard';
import { useCategories, useCategoryName, useCategoryUsage } from '@/hooks/use-categories';
import { useCategoryMutations } from '@/hooks/use-category-mutations';
import type { CategoryUsage, TransactionCategory } from '@shared/schema';

const EMPTY_USAGE: Omit<CategoryUsage, 'categoryId'> = {
  transactions: 0,
  rules: 0,
  learnedPayees: 0,
  budgetGoals: 0,
};

function totalUsage(usage: Omit<CategoryUsage, 'categoryId'>): number {
  return usage.transactions + usage.rules + usage.learnedPayees + usage.budgetGoals;
}

/**
 * Settings → Categorization → Kategorie: one row per category with
 * icon, name, system badge and usage; add, rename / restyle, and delete with
 * a reassign target. System categories can be edited but not deleted; the
 * backend enforces the same rules.
 */
export function CategoriesCard() {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const { categories } = useCategories();
  const { usage } = useCategoryUsage();
  const categoryName = useCategoryName();
  const { deleteCategory } = useCategoryMutations();

  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<TransactionCategory | null>(null);
  const [deleting, setDeleting] = useState<TransactionCategory | null>(null);
  const [reassignTo, setReassignTo] = useState('');

  const usageById = new Map(usage.map((u) => [u.categoryId, u]));
  const usageOf = (id: string) => usageById.get(id) ?? EMPTY_USAGE;

  const closeDelete = () => {
    setDeleting(null);
    setReassignTo('');
  };

  const deletingUsage = deleting ? usageOf(deleting.id) : EMPTY_USAGE;
  const deletingInUse = totalUsage(deletingUsage) > 0;
  const canConfirmDelete = deleting !== null && (!deletingInUse || reassignTo !== '');

  const handleConfirmDelete = async () => {
    if (!deleting || !canConfirmDelete) return;
    try {
      await deleteCategory.mutateAsync({
        id: deleting.id,
        reassignTo: deletingInUse ? reassignTo : null,
      });
      closeDelete();
    } catch {
      // onError already showed a translated toast; keep the dialog open.
    }
  };

  const describeUsage = (u: Omit<CategoryUsage, 'categoryId'>) =>
    t('categoryManagement.usageBreakdown', {
      transactions: u.transactions,
      rules: u.rules,
      learnedPayees: u.learnedPayees,
      budgetGoals: u.budgetGoals,
    });

  return (
    <SettingsCard
      title={t('categoryManagement.title')}
      description={t('categoryManagement.description')}
      action={
        <Button size="sm" onClick={() => setCreating(true)}>
          <Plus />
          {t('categoryManagement.add')}
        </Button>
      }
    >
      <div className="-mr-2 max-h-[440px] overflow-y-auto pr-2">
        {categories.map((category) => {
          const u = usageOf(category.id);
          const color = category.color || '#9E9E9E';
          return (
            <div
              key={category.id}
              className="group/cat flex items-center justify-between gap-3 border-b border-line-soft py-2 last:border-b-0"
            >
              <div className="flex min-w-0 items-center gap-3">
                <span
                  className="grid size-7 shrink-0 place-items-center rounded-full [&_svg]:size-[15px]"
                  style={{ backgroundColor: `${color}26`, color }}
                >
                  <CategoryIcon iconName={category.icon || 'tag'} />
                </span>
                <span className="truncate text-body font-600 text-ink">
                  {categoryName(category)}
                </span>
                {category.isSystem && (
                  <Badge variant="outline">{t('categoryManagement.system')}</Badge>
                )}
              </div>
              <div className="flex shrink-0 items-center gap-1">
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span className="mr-2 text-caption text-ink-4 num">
                      {t('categoryManagement.transactionsCount', { n: u.transactions })}
                    </span>
                  </TooltipTrigger>
                  <TooltipContent>{describeUsage(u)}</TooltipContent>
                </Tooltip>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={tc('buttons.edit')}
                  onClick={() => setEditing(category)}
                >
                  <Pencil />
                </Button>
                <Tooltip>
                  <TooltipTrigger asChild>
                    {/* A disabled button swallows hover; the span carries the tooltip. */}
                    <span>
                      <Button
                        variant={category.isSystem ? 'ghost' : 'danger'}
                        size="icon-sm"
                        aria-label={tc('buttons.delete')}
                        disabled={category.isSystem}
                        onClick={() => setDeleting(category)}
                      >
                        <Trash2 />
                      </Button>
                    </span>
                  </TooltipTrigger>
                  {category.isSystem && (
                    <TooltipContent>{t('categoryManagement.systemCannotDelete')}</TooltipContent>
                  )}
                </Tooltip>
              </div>
            </div>
          );
        })}
      </div>

      {creating && (
        <CategoryFormDialog
          open
          onOpenChange={(open) => {
            if (!open) setCreating(false);
          }}
        />
      )}
      {editing && (
        <CategoryFormDialog
          key={editing.id}
          open
          category={editing}
          onOpenChange={(open) => {
            if (!open) setEditing(null);
          }}
        />
      )}

      <AlertDialog
        open={deleting !== null}
        onOpenChange={(open) => {
          if (!open) closeDelete();
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t('categoryManagement.deleteTitle', {
                name: deleting ? categoryName(deleting) : '',
              })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {deletingInUse
                ? t('categoryManagement.deleteInUseDescription', {
                    usage: describeUsage(deletingUsage),
                  })
                : t('categoryManagement.deleteUnusedDescription')}
            </AlertDialogDescription>
          </AlertDialogHeader>

          {deletingInUse && (
            <div className="space-y-2">
              <Label htmlFor="reassign-category">{t('categoryManagement.moveTo')}</Label>
              <Select value={reassignTo} onValueChange={setReassignTo}>
                <SelectTrigger id="reassign-category">
                  <SelectValue placeholder={t('categoryManagement.moveToPlaceholder')} />
                </SelectTrigger>
                <SelectContent>
                  {categories
                    .filter((c) => c.id !== deleting?.id)
                    .map((c) => (
                      <SelectItem key={c.id} value={c.id}>
                        {categoryName(c)}
                      </SelectItem>
                    ))}
                </SelectContent>
              </Select>
            </div>
          )}

          <AlertDialogFooter>
            <AlertDialogCancel>{tc('buttons.cancel')}</AlertDialogCancel>
            {/* A plain Button: AlertDialogAction would close the dialog before the delete finishes. */}
            <Button
              variant="destructive"
              disabled={!canConfirmDelete}
              loading={deleteCategory.isPending}
              onClick={() => void handleConfirmDelete()}
            >
              {t('categoryManagement.deleteConfirm')}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </SettingsCard>
  );
}
