import type { ReactNode } from 'react';
import { ArrowDown, ArrowUp, ArrowUpDown } from 'lucide-react';
import { cn } from '@/lib/utils';
import { TableHead } from '@/components/ui/table';

/**
 * A table column head that sorts by its column (design system §7 tables): the label and an
 * arrow that shows the active column and its direction.
 *
 * A module-level component on purpose: a head declared inside the table's render was a new
 * component type on every render, so React remounted the button after each sort and keyboard
 * focus fell back to the page.
 */
export function SortableTableHead<C extends string>({
  column,
  sortColumn,
  sortDirection,
  onSort,
  align = 'right',
  truncate = false,
  className,
  children,
}: {
  column: C;
  sortColumn: C;
  sortDirection: 'asc' | 'desc';
  onSort: (column: C) => void;
  align?: 'left' | 'right';
  /** Clips a long label with an ellipsis; a text label then shows in full on hover. */
  truncate?: boolean;
  className?: string;
  children: ReactNode;
}) {
  const active = sortColumn === column;
  const Icon = !active ? ArrowUpDown : sortDirection === 'asc' ? ArrowUp : ArrowDown;
  return (
    <TableHead
      className={cn(align === 'right' && 'text-right', className)}
      // On the column header, where assistive technology reads it (not on the button).
      aria-sort={active ? (sortDirection === 'asc' ? 'ascending' : 'descending') : undefined}
    >
      <button
        type="button"
        onClick={() => onSort(column)}
        title={truncate && typeof children === 'string' ? children : undefined}
        className={cn(
          'inline-flex items-center gap-1 rounded-r1 text-thead uppercase transition-colors duration-fast hover:text-ink focus-visible:outline-none focus-visible:shadow-focus',
          truncate && 'max-w-full',
          active && 'text-ink'
        )}
      >
        {truncate ? (
          // leading-4: the ellipsis clip must not cut the accents of Czech capitals (Í, Š, Ř)
          <span className="truncate leading-4">{children}</span>
        ) : (
          children
        )}
        <Icon
          className={cn('size-3', truncate && 'shrink-0', !active && 'opacity-60')}
          aria-hidden
        />
      </button>
    </TableHead>
  );
}
