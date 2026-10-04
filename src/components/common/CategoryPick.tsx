import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { TransactionCategory } from '@shared/schema';
import { categoryDisplayName } from '@/utils/category-name';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command';
import { CategoryIcon } from '@/components/common/CategoryIcon';
import { cn } from '@/lib/utils';

interface CategoryPickProps {
  categories: readonly TransactionCategory[];
  selectedId: string | null;
  /** Offer "no category" as the first entry (drops an override). */
  allowClear?: boolean;
  onPick: (categoryId: string | null) => void;
  /** The trigger: a chip or a dashed "pick" button. */
  children: ReactNode;
  align?: 'start' | 'end';
}

/**
 * Category picker in a popover (design system §6 Popover): a searchable list
 * with the category icon, used inline in tables (CSV preview, learned rules).
 */
export function CategoryPick({
  categories,
  selectedId,
  allowClear = false,
  onPick,
  children,
  align = 'start',
}: CategoryPickProps) {
  const { t } = useTranslation('bank_accounts');
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent className="w-[240px] p-0" align={align}>
        <Command>
          <CommandInput placeholder={t('csvImport.preview.searchCategory')} />
          <CommandList className="max-h-[260px]">
            <CommandEmpty>—</CommandEmpty>
            <CommandGroup>
              {allowClear && (
                <CommandItem
                  value="__none__"
                  onSelect={() => {
                    onPick(null);
                    setOpen(false);
                  }}
                  className="text-ink-3"
                >
                  {t('csvImport.preview.noCategory')}
                </CommandItem>
              )}
              {categories.map((category) => (
                <CommandItem
                  key={category.id}
                  value={`${categoryDisplayName(category, t)} ${category.id}`}
                  onSelect={() => {
                    onPick(category.id);
                    setOpen(false);
                  }}
                  className={cn(category.id === selectedId && 'font-650')}
                >
                  <CategoryIcon iconName={category.icon ?? 'tag'} className="size-3.5 text-ink-3" />
                  {categoryDisplayName(category, t)}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

/** The category as a dot + name chip; `note` is the quiet suffix ("· naučené"). */
export function CategoryChip({
  category,
  note,
  className,
}: {
  category: TransactionCategory;
  note?: string | null;
  className?: string;
}) {
  const { t } = useTranslation('bank_accounts');
  return (
    <span
      className={cn('inline-flex max-w-full items-center gap-1.5 font-600 text-ink', className)}
    >
      <i
        className="block size-[7px] shrink-0 rounded-full"
        style={{ background: category.color || 'var(--s2)' }}
      />
      <span className="truncate">{categoryDisplayName(category, t)}</span>
      {note && <span className="shrink-0 font-500 text-ink-4">· {note}</span>}
    </span>
  );
}
