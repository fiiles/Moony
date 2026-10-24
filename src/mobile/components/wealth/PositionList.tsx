import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Search } from 'lucide-react';
import type { PositionRowBase } from '@/utils/positions';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SearchField } from '@/mobile/components/SearchField';
import { useRemembered } from '@/mobile/hooks/use-remembered';
import { displayName, foldRows, positionRows, type PositionSort } from '@/mobile/lib/position-rows';
import { NavIconButton } from '@/mobile/shell/NavBar';
import { MoreRow } from './MoreRow';
import { PositionRow } from './PositionRow';
import { SortPill } from './SortPill';

interface PositionListProps<R extends PositionRowBase & { companyName?: string; name?: string }> {
  rows: readonly R[];
  /** Where a tap on a row goes. */
  hrefOf: (row: R) => string;
  /** The report currency: the value and the gain of a row are the backend's, converted into it. */
  currency: string;
  showClosed: boolean;
  /** A refresh is running: the list is dimmed. */
  dimmed: boolean;
  /** The sort is remembered for the session under `${rememberKey}.sort`. */
  rememberKey: string;
}

/**
 * The positions of a class: a toolbar (the sort pill and the search toggle), the search field
 * while it is on, then the positions as rows — the first eight, "Zobrazit všech N" for the rest.
 * Closed positions wait behind the `···` menu. A search that finds nothing says so instead of
 * showing an empty list.
 */
export function PositionList<R extends PositionRowBase & { companyName?: string; name?: string }>({
  rows,
  hrefOf,
  currency,
  showClosed,
  dimmed,
  rememberKey,
}: PositionListProps<R>) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const [sort, setSort] = useRemembered<PositionSort>(`${rememberKey}.sort`, 'value');
  const [search, setSearch] = useState('');
  const [searchOpen, setSearchOpen] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const searchBox = useRef<HTMLDivElement>(null);

  // A search that is out of sight must not keep filtering the list
  const toggleSearch = () => {
    if (searchOpen) setSearch('');
    setSearchOpen(!searchOpen);
  };

  // Opening the search is for typing: the field takes the focus (and the keyboard) at once
  useEffect(() => {
    if (searchOpen) searchBox.current?.querySelector('input')?.focus();
  }, [searchOpen]);

  const shownRows = positionRows(rows, { sort, search, showClosed });
  const { shown, hidden } = foldRows(shownRows, expanded);

  return (
    <>
      <div className="mb-3 flex items-center justify-between gap-2.5">
        <SortPill
          value={sort}
          options={[
            { value: 'value', label: t('wealth.stocks.sort.value') },
            { value: 'gain', label: t('wealth.stocks.sort.gain') },
            { value: 'name', label: t('wealth.stocks.sort.name') },
          ]}
          onChange={setSort}
          label={t('wealth.stocks.sort.label')}
        />
        <NavIconButton
          aria-label={t('wealth.stocks.searchToggle')}
          aria-pressed={searchOpen}
          onClick={toggleSearch}
          className="border border-line-strong bg-paper shadow-btn [&_svg]:size-5"
        >
          <Search strokeWidth={1.75} aria-hidden />
        </NavIconButton>
      </div>
      {searchOpen && (
        <div ref={searchBox}>
          <SearchField
            value={search}
            onChange={setSearch}
            placeholder={ts('table.search')}
            clearLabel={t('wealth.stocks.clearSearch')}
          />
        </div>
      )}
      {shownRows.length === 0 ? (
        <p className="mx-0.5 py-6 text-center text-m-body text-ink-3">
          {search.trim() === '' ? ts('table.onlyClosedPositions') : ts('table.noResults.title')}
        </p>
      ) : (
        <EntityRows className={dimmed ? 'opacity-50 transition-opacity' : undefined}>
          {shown.map((row) => (
            <PositionRow
              key={row.id}
              row={row}
              name={displayName(row)}
              href={hrefOf(row)}
              currency={currency}
            />
          ))}
          {hidden > 0 && (
            <MoreRow
              label={t('wealth.stocks.showAll', { count: shownRows.length })}
              onClick={() => setExpanded(true)}
            />
          )}
        </EntityRows>
      )}
    </>
  );
}
