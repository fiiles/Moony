import { useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Pencil, Plus, Trash2 } from 'lucide-react';
import type { CategoryUsage, TransactionCategory } from '@shared/schema';
import { CategoryFormDialog } from '@/components/categories/CategoryFormDialog';
import { useCategories, useCategoryName, useCategoryUsage } from '@/hooks/use-categories';
import { CategoryChip } from '@/mobile/components/cashflow/CategoryChip';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { ScaledReportError } from '@/mobile/components/Scaled';
import { SectionHead } from '@/mobile/components/SectionHead';
import { SectionPage } from '@/mobile/components/SectionPage';
import { renderTextRef } from '@/mobile/lib/settings-summary';
import {
  categoryRowActions,
  groupCategories,
  usageParts,
  type CategoryRow,
} from '@/mobile/lib/category-rows';
import { NavIconButton } from '@/mobile/shell/NavBar';
import { CategoryDeleteSheet } from '@/mobile/sheets/CategoryDeleteSheet';

/**
 * Nastavení → Kategorie: the categories grouped by kind, each with its usage. Tap edits (the
 * desktop's form, as a sheet), `+` adds; swipe left and the long press's menu delete — for a
 * system category the menu only says why not. A category in use asks where to move everything
 * before it goes. Same queries and mutations as the desktop manager, so both share one cache.
 */
export default function CategoriesSection() {
  const { t } = useTranslation('bank_accounts');
  const { categories, isLoading, isError, refetch } = useCategories();
  const {
    usage,
    isLoading: usageLoading,
    isError: usageError,
    refetch: refetchUsage,
  } = useCategoryUsage();
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<TransactionCategory | null>(null);
  const [deleting, setDeleting] = useState<CategoryRow | null>(null);

  const groups = useMemo(() => groupCategories(categories, usage), [categories, usage]);
  const describeUsage = (u: Omit<CategoryUsage, 'categoryId'>) =>
    t('categoryManagement.usageBreakdown', {
      transactions: u.transactions,
      rules: u.rules,
      learnedPayees: u.learnedPayees,
      budgetGoals: u.budgetGoals,
    });

  let body: ReactNode;
  // Without the usage every category would read as unused, so a failed usage query is an error too
  if (isError || usageError) {
    body = (
      <ScaledReportError
        onRetry={() => {
          if (isError) void refetch();
          if (usageError) void refetchUsage();
        }}
      />
    );
  } else if (isLoading || usageLoading) {
    body = <CategoriesSkeleton />;
  } else {
    body = groups.map((group) => (
      <section key={group.kind}>
        <SectionHead title={t(`readiness.flow.${group.kind}`)} />
        <EntityRows>
          {group.rows.map((row) => (
            <CategoryItem
              key={row.category.id}
              row={row}
              onEdit={() => setEditing(row.category)}
              onDelete={() => setDeleting(row)}
            />
          ))}
        </EntityRows>
      </section>
    ));
  }

  return (
    <SectionPage
      title={t('categoryManagement.title')}
      lead={t('categoryManagement.description')}
      trailing={
        <NavIconButton aria-label={t('categoryManagement.add')} onClick={() => setCreating(true)}>
          <Plus strokeWidth={1.75} aria-hidden />
        </NavIconButton>
      }
    >
      {body}
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
      <CategoryDeleteSheet
        row={deleting}
        usageText={deleting?.usage ? describeUsage(deleting.usage) : ''}
        onClose={() => setDeleting(null)}
      />
    </SectionPage>
  );
}

interface CategoryItemProps {
  row: CategoryRow;
  onEdit: () => void;
  onDelete: () => void;
}

/** One category: chip, name, usage line and the built-in label; the row's gestures as the budgets'. */
function CategoryItem({ row, onEdit, onDelete }: CategoryItemProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tm } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const categoryName = useCategoryName();
  const actions = categoryRowActions(row);
  const name = categoryName(row.category);

  // What holds the category, the non-zero parts only ("16 transakcí · 1 rozpočet")
  const usageLine = row.usage
    ? usageParts(row.usage)
        .map((part) => renderTextRef(tm, part))
        .join(' · ') || undefined
    : undefined;

  return (
    <EntityRow
      chip={<CategoryChip category={row.category} />}
      title={name}
      sub={usageLine}
      valueSub={row.category.isSystem ? t('categoryManagement.system') : undefined}
      onClick={onEdit}
      onSwipeLeft={actions.delete ? onDelete : undefined}
      swipeLeftLabel={tc('buttons.delete')}
      swipeLeftIcon={Trash2}
      swipeLeftDestructive
      menu={{
        label: tm('cashflow.rowMenu', { title: name }),
        // Smazat is nowhere else: the row's tap edits the category
        shown: true,
        children: (
          <>
            <RowMenuItem onSelect={onEdit}>
              <Pencil aria-hidden strokeWidth={1.75} />
              {tc('buttons.edit')}
            </RowMenuItem>
            {actions.delete ? (
              <RowMenuItem variant="danger" onSelect={onDelete}>
                <Trash2 aria-hidden strokeWidth={1.75} />
                {tc('buttons.delete')}
              </RowMenuItem>
            ) : (
              <RowMenuItem disabled>
                <Trash2 aria-hidden strokeWidth={1.75} />
                {t('categoryManagement.systemCannotDelete')}
              </RowMenuItem>
            )}
          </>
        ),
      }}
    />
  );
}

/** While the categories load for the first time: rows in outline. */
function CategoriesSkeleton() {
  return (
    <div aria-hidden>
      <EntityRows>
        {Array.from({ length: 6 }, (_, i) => (
          <li
            key={i}
            className="flex min-h-[52px] list-none items-center gap-3 border-t border-line-soft px-3.5 first:border-t-0"
          >
            <div className="skeleton size-9 flex-none rounded-full" />
            <div className="skeleton h-3.5 w-2/5" />
          </li>
        ))}
      </EntityRows>
    </div>
  );
}
