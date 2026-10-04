import { useEffect, useMemo, useRef, useState } from 'react';
import { stockImportApi } from '@/lib/tauri-api';
import type {
  StockCurrencyMode,
  StockImportInstrument,
  StockInstrumentResolution,
} from '@shared/schema';
import { EMPTY_LOOKUP_STATE, createInstrumentLookup, type LookupState } from './instrument-lookup';
import { needsLookup } from './import-config';

interface UseInstrumentResolutionOptions {
  /** Looking up starts when the review step is open, not before. */
  enabled: boolean;
  /** The instruments of the latest preview. */
  instruments: readonly StockImportInstrument[] | undefined;
  /** Where the currency of the trades comes from: with `instrument`, positions are looked up too. */
  currencyMode: StockCurrencyMode;
  /** Called with every answer and the instruments it is about (to apply what was found). */
  onResolved: (
    instruments: readonly StockImportInstrument[],
    resolutions: readonly StockInstrumentResolution[]
  ) => void;
}

/**
 * Confirms the instruments of a file on Yahoo Finance (`resolveInstruments`):
 * once per instrument and file, in the background of the review, with progress.
 * A lookup that fails (offline, rate limit) marks the instrument unverified and
 * never blocks anything; "skip" stops waiting. The queue itself is plain TypeScript
 * (`instrument-lookup.ts`); this is the React side of it.
 */
export function useInstrumentResolution({
  enabled,
  instruments,
  currencyMode,
  onResolved,
}: UseInstrumentResolutionOptions) {
  const [state, setState] = useState<LookupState>(EMPTY_LOOKUP_STATE);
  const onResolvedRef = useRef(onResolved);
  const instrumentsRef = useRef(instruments);
  useEffect(() => {
    onResolvedRef.current = onResolved;
    instrumentsRef.current = instruments;
  });

  const lookup = useMemo(
    () =>
      createInstrumentLookup({
        resolve: (queries) => stockImportApi.resolveInstruments(queries),
        onChange: setState,
        onResolved: (asked, answers) => onResolvedRef.current(asked, answers),
      }),
    []
  );

  // Every instrument that is new or has no symbol is looked up once per file (and, when the
  // currency comes from the listing, the existing positions too).
  useEffect(() => {
    if (!enabled || !instruments) return;
    lookup.enqueue(instruments.filter((instrument) => needsLookup(instrument, currencyMode)));
  }, [enabled, instruments, currencyMode, lookup]);

  return {
    resolutions: state.resolutions,
    progress: state.progress,
    isResolving: state.progress.done < state.progress.total,
    skip: lookup.skip,
    retry: () => lookup.retryFailed(instrumentsRef.current ?? []),
    reset: lookup.reset,
    remember: lookup.remember,
    verify: lookup.verify,
  };
}
