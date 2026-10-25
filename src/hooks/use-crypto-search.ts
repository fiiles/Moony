import { useQuery } from '@tanstack/react-query';
import { priceApi, type CryptoSearchResult } from '@/lib/tauri-api';
import { useDebouncedValue } from '@/hooks/use-debounced-value';

/** Pause after the last keystroke before the Yahoo Finance search starts. */
const SEARCH_DEBOUNCE_MS = 400;
const MIN_AUTO_SEARCH_CHARS = 2;
const SEARCH_STALE_MS = 5 * 60 * 1000;

export interface CryptoSearchState {
  /** The list has something to say (a query long enough, the dialog wants suggestions). */
  show: boolean;
  /** Typing has not settled yet, or the request is in flight. */
  waiting: boolean;
  results: CryptoSearchResult[];
  failed: boolean;
}

/**
 * The Yahoo Finance coin search both crypto dialogs share: typed text searches after a 400 ms
 * pause once it is two characters long; `forced` (the search button) searches at once from one
 * character; results stay fresh for five minutes. `enabled` lets a dialog hold the list back
 * (closed, or a result was just picked).
 */
export function useCryptoSearch({
  typed,
  forced = null,
  enabled = true,
}: {
  typed: string;
  forced?: string | null;
  enabled?: boolean;
}): CryptoSearchState {
  const debounced = useDebouncedValue(typed, SEARCH_DEBOUNCE_MS);
  const queryText = (forced ?? typed).trim();
  const minChars = forced === null ? MIN_AUTO_SEARCH_CHARS : 1;
  const show = enabled && queryText.length >= minChars;
  const typingPending = forced === null && debounced !== typed;

  const {
    data: results = [],
    isFetching,
    isError,
  } = useQuery<CryptoSearchResult[]>({
    queryKey: ['crypto-search', queryText],
    queryFn: () => priceApi.searchCrypto(queryText),
    enabled: show && !typingPending,
    staleTime: SEARCH_STALE_MS,
  });

  return { show, waiting: typingPending || isFetching, results, failed: isError };
}
