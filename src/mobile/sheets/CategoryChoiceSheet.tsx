import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Tag } from 'lucide-react';
import type { TransactionCategory } from '@shared/schema';
import { CategoryIconChip } from '@/components/common/CategoryIcon';
import { Button } from '@/components/ui/button';
import { useCategoryName } from '@/hooks/use-categories';
import { NeutralChip, PickGroup, PickRow } from '@/mobile/components/PickRow';
import { SearchField } from '@/mobile/components/SearchField';
import { Sheet } from '@/mobile/components/Sheet';
import { filterCategories } from '@/mobile/lib/category-pick';

interface CategoryChoiceSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  categories: readonly TransactionCategory[];
  /** The category chosen so far; `null` = none chosen, the rules decide. */
  value: string | null;
  /** The choice; `null` takes the choice back. Nothing is saved and nothing is learned here. */
  onChoose: (categoryId: string | null) => void;
}

/**
 * A plain choice of a category (README §13): the search, "Podle pravidel" first (the way back from
 * a choice), then the categories with a check on the chosen one; a tap chooses and closes. Unlike
 * the category picker it writes nothing and learns nothing — the import preview keeps the choice
 * until the import is written (a stacked sheet over the import sheet).
 */
export function CategoryChoiceSheet({
  open,
  onOpenChange,
  categories,
  value,
  onChoose,
}: CategoryChoiceSheetProps) {
  const { t: tc } = useTranslation('common');
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const categoryName = useCategoryName();
  const [query, setQuery] = useState('');
  // The search starts over every time the sheet opens (not while it slides away)
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) setQuery('');
  }
  const searching = query.trim() !== '';
  const shown = filterCategories(categories, query, categoryName);
  const choose = (categoryId: string | null) => {
    onChoose(categoryId);
    onOpenChange(false);
  };

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('picker.title')}
      full
      footer={
        <Button
          variant="ghost"
          className="h-12 flex-1 rounded-[10px] text-m-row"
          onClick={() => onOpenChange(false)}
        >
          {tc('buttons.cancel')}
        </Button>
      }
    >
      <SearchField
        value={query}
        onChange={setQuery}
        placeholder={tb('categorization.searchCategory')}
        clearLabel={tc('a11y.clearSearch')}
      />
      {!searching && (
        <PickGroup title={t('import.choice.title')}>
          <PickRow
            icon={<NeutralChip icon={<Tag />} />}
            label={t('import.choice.byRules')}
            sub={t('import.choice.byRulesSub')}
            checked={value === null}
            onSelect={() => choose(null)}
          />
        </PickGroup>
      )}
      {shown.length > 0 ? (
        <PickGroup title={t('picker.all')}>
          {shown.map((category) => (
            <PickRow
              key={category.id}
              icon={<CategoryIconChip category={category} />}
              label={categoryName(category)}
              checked={value === category.id}
              onSelect={() => choose(category.id)}
            />
          ))}
        </PickGroup>
      ) : (
        <p className="mx-1 my-3 text-m-body text-ink-3">{t('picker.empty')}</p>
      )}
    </Sheet>
  );
}
