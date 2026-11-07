import { useTranslation } from 'react-i18next';
import { X } from 'lucide-react';
import type { LearnedPayeeEntry, TransactionCategory } from '@shared/schema';
import { useCategoryName } from '@/hooks/use-categories';
import { CategoryChip } from '@/mobile/components/cashflow/CategoryChip';
import { EntityRow } from '@/mobile/components/EntityRow';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { learnedPayeeScope, learnedPayeeTitle } from '@/mobile/lib/categorization-rows';

interface LearnedPayeeRowProps {
  entry: LearnedPayeeEntry;
  category: TransactionCategory | undefined;
  accountName: (id: string) => string | null;
  onForget: () => void;
}

/** One learned payee: its name (or IBAN), the category and where the rule applies; "Zapomenout" by swipe or long press. */
export function LearnedPayeeRow({ entry, category, accountName, onForget }: LearnedPayeeRowProps) {
  const { t } = useTranslation('categorization');
  const { t: tm } = useTranslation('mobile');
  const categoryName = useCategoryName();
  const title = learnedPayeeTitle(entry);
  const scope = learnedPayeeScope(entry, accountName);

  let scopeLine: string;
  if (scope.kind === 'legacy') {
    scopeLine = t('legacyLearning');
  } else {
    const account = scope.account ?? tm('settings.categorization.deletedAccount');
    scopeLine = scope.direction
      ? t('scopedLearning', { account, direction: t(scope.direction) })
      : tm('settings.categorization.scopedNoDirection', { account });
  }
  // The payee as the bank wrote it, when it differs from the title (the desktop's second line)
  const original =
    entry.originalPayee && entry.originalPayee !== title ? entry.originalPayee : null;
  const categoryLabel = category ? categoryName(category) : entry.categoryId;

  return (
    <EntityRow
      chip={<CategoryChip category={category} />}
      title={title}
      sub={`${scopeLine} · ${categoryLabel}`}
      below={
        original ? (
          <p className="m-0 truncate pb-2 pl-[62px] pr-3.5 text-m-caption text-ink-4">{original}</p>
        ) : undefined
      }
      onSwipeLeft={onForget}
      swipeLeftLabel={t('learnedTab.forget')}
      swipeLeftIcon={X}
      swipeLeftDestructive
      menu={{
        label: tm('cashflow.rowMenu', { title }),
        // Zapomenout is nowhere else on the row
        shown: true,
        children: (
          <>
            <RowMenuItem variant="danger" onSelect={onForget}>
              <X aria-hidden strokeWidth={1.75} />
              {t('learnedTab.forget')}
            </RowMenuItem>
          </>
        ),
      }}
    />
  );
}
