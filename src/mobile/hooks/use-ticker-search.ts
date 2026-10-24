import { useEffect, useState } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { priceApi, type StockSearchResult } from '@/lib/tauri-api';

/** The desktop search's rules (`StockSearchCombobox`): from two characters, after a pause in typing. */
const MIN_CHARS = 2;
const DEBOUNCE_MS = 300;

/**
 * Yahoo's ticker search for the phone's Sledované field. Nothing is asked under two characters,
 * and a query goes out 300 ms after the last keystroke (`['stock-search', term]`, so a term
 * typed before is answered from the cache). `results` are those of the last term that went out —
 * the previous ones stay while the next are on their way (`keepPreviousData`); `isFetching` says
 * that what is typed has no answer yet (the pause included), so "no results" is never shown for a
 * field that has not been asked. A failed search counts as no results, as on the desktop.
 */
export function useTickerSearch(query: string): {
  results: StockSearchResult[];
  isFetching: boolean;
} {
  const term = query.trim();
  const active = term.length >= MIN_CHARS;
  const [debounced, setDebounced] = useState('');

  // Under two characters nothing is searched and the term that was asked is dropped at once, so
  // a field under two characters shows no hits
  if (!active && debounced !== '') setDebounced('');

  useEffect(() => {
    if (!active) return;
    const handle = setTimeout(() => setDebounced(term), DEBOUNCE_MS);
    return () => clearTimeout(handle);
  }, [active, term]);

  const search = useQuery({
    queryKey: ['stock-search', debounced],
    queryFn: () => priceApi.searchStockTickers(debounced),
    enabled: debounced !== '',
    staleTime: 5 * 60 * 1000,
    // The last hits stay (dimmed by the page) while the next term is on its way
    placeholderData: keepPreviousData,
  });

  return {
    results: active && debounced !== '' ? (search.data ?? []) : [],
    isFetching: active && (debounced !== term || search.isFetching),
  };
}
