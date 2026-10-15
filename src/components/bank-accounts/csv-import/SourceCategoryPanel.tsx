import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown } from 'lucide-react';
import type { SourceCategoryCount, TransactionCategory } from '@shared/schema';
import { CategoryPick } from '@/components/common/CategoryPick';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { categoryDisplayName } from '@/utils/category-name';
import { cn } from '@/lib/utils';

interface SourceCategoryPanelProps {
  /** The categories the statement's payments carry in the file (QIF `L`), with their counts. */
  sources: readonly SourceCategoryCount[];
  /** Source path → the Moony category chosen for it; null = left to the rules. */
  chosen: Readonly<Record<string, string | null>>;
  onPick: (source: string, categoryId: string | null) => void;
  categories: readonly TransactionCategory[];
}

/**
 * "Categories from the file" (spec §8.2, D13): every source category of the statement with
 * its payment count and the Moony category its payments get, prefilled by name. A source left
 * to the rules goes through categorization like any CSV row; a pick in the table below wins
 * over the one made here.
 */
export function SourceCategoryPanel({
  sources,
  chosen,
  onPick,
  categories,
}: SourceCategoryPanelProps) {
  const { t } = useTranslation('bank_accounts');
  const [open, setOpen] = useState(true);
  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);

  return (
    <Collapsible
      open={open}
      onOpenChange={setOpen}
      className="mb-3.5 rounded-r3 border border-line"
    >
      <CollapsibleTrigger asChild>
        <button
          type="button"
          className="flex w-full items-center gap-2 rounded-r3 px-3.5 py-3 text-left text-caption font-600 text-ink-2 hover:text-ink"
        >
          <ChevronDown
            className={cn(
              'size-3.5 shrink-0 text-ink-4 transition-transform duration-base',
              !open && '-rotate-90'
            )}
            aria-hidden
          />
          {t('statementImport.categories.title')}
        </button>
      </CollapsibleTrigger>
      <CollapsibleContent>
        <div className="border-t border-line px-3.5 pb-3 pt-2.5">
          <p className="mb-2 mt-0 text-micro font-500 text-ink-4">
            {t('statementImport.categories.hint')}
          </p>
          <ul className="m-0 grid list-none gap-1 p-0">
            {sources.map((source) => {
              const categoryId = chosen[source.name] ?? null;
              const category = categoryId ? categoryById.get(categoryId) : undefined;
              return (
                <li key={source.name} className="flex min-h-8 items-center gap-3 text-caption">
                  <span className="min-w-0 flex-1 truncate font-600 text-ink" title={source.name}>
                    {source.name}
                  </span>
                  <span className="shrink-0 text-micro font-500 text-ink-4">
                    {t('statementImport.categories.count', { count: source.count })}
                  </span>
                  <span className="flex w-[38%] min-w-0 shrink-0">
                    <CategoryPick
                      categories={categories}
                      selectedId={category?.id ?? null}
                      allowClear={category != null}
                      clearLabel={t('statementImport.categories.rules')}
                      onPick={(id) => onPick(source.name, id)}
                      align="end"
                    >
                      {category ? (
                        <button
                          type="button"
                          className="inline-flex max-w-full items-center gap-1.5 rounded-r1 text-left font-600 text-ink hover:text-ink-2"
                        >
                          <i
                            className="block size-[7px] shrink-0 rounded-full"
                            style={{ background: category.color || 'var(--s2)' }}
                          />
                          <span className="truncate">{categoryDisplayName(category, t)}</span>
                        </button>
                      ) : (
                        <button
                          type="button"
                          className="inline-flex max-w-full items-center truncate rounded-[6px] border border-dashed border-line-strong bg-paper px-[7px] py-0.5 text-micro font-600 text-ink-3 hover:border-ink-3 hover:text-ink"
                        >
                          {t('statementImport.categories.rules')}
                        </button>
                      )}
                    </CategoryPick>
                  </span>
                </li>
              );
            })}
          </ul>
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}
