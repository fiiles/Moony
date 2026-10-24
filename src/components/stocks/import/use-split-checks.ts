import { useEffect, useMemo, useRef, useState } from 'react';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { stockImportApi } from '@/lib/tauri-api';
import type { StockImportConfig, StockImportInstrument, StockSplitCheck } from '@shared/schema';
import { PREVIEW_DEBOUNCE_MS } from './import-config';

interface UseSplitChecksOptions {
  /** The review step is open. */
  enabled: boolean;
  /** Nothing else is settling: the lookups are done and the preview is current. */
  ready: boolean;
  filePath: string;
  /** The config of the import; the backend leaves its split choices out of the check (D12). */
  config: StockImportConfig | null;
  /** The instruments of the latest preview. */
  instruments: readonly StockImportInstrument[] | undefined;
  /** Called with every check that arrives, to turn it into split choices. */
  onChecked: (checks: readonly StockSplitCheck[]) => void;
}

interface CheckState {
  /** The ticker signature the checks below are for; null before the first one. */
  signature: string | null;
  checks: Record<string, StockSplitCheck>;
  error: unknown;
}

const EMPTY_STATE: CheckState = { signature: null, checks: {}, error: null };

/**
 * The splits Yahoo reports for the file's trades (`checkSplits`, spec D12): once the instruments
 * are resolved, then again only when what the trades would be stored under changes — an
 * instrument's ticker or whether it is skipped — debounced like the preview. A response that is
 * no longer the latest is dropped; a failed check keeps the previous checks on screen and does
 * not block the import.
 */
export function useSplitChecks({
  enabled,
  ready,
  filePath,
  config,
  instruments,
  onChecked,
}: UseSplitChecksOptions) {
  const [state, setState] = useState<CheckState>(EMPTY_STATE);
  const configRef = useRef(config);
  const onCheckedRef = useRef(onChecked);
  useEffect(() => {
    configRef.current = config;
    onCheckedRef.current = onChecked;
  });
  // Bumped per request and by a reset; a response that is no longer the latest is dropped.
  const request = useRef(0);
  // The signature of the request in flight, so a re-render does not ask twice.
  const inFlight = useRef<string | null>(null);

  const signature = useMemo(
    () =>
      enabled && instruments
        ? instruments
            .map((i) => `${i.key}:${i.status === 'skipped' ? '-' : (i.ticker ?? '')}`)
            .join('|')
        : null,
    [enabled, instruments]
  );
  const debounced = useDebouncedValue(signature, PREVIEW_DEBOUNCE_MS);

  useEffect(() => {
    const current = configRef.current;
    if (!ready || !current || signature == null || debounced !== signature) return;
    if (signature === state.signature || signature === inFlight.current) return;
    inFlight.current = signature;
    const requestId = ++request.current;
    stockImportApi.checkSplits(filePath, current).then(
      (checks) => {
        if (requestId !== request.current) return;
        inFlight.current = null;
        setState({
          signature,
          checks: Object.fromEntries(checks.map((check) => [check.key, check])),
          error: null,
        });
        onCheckedRef.current(checks);
      },
      (error: unknown) => {
        if (requestId !== request.current) return;
        inFlight.current = null;
        console.error('Split check failed:', error);
        setState((previous) => ({ ...previous, signature, error }));
      }
    );
  }, [ready, filePath, signature, debounced, state.signature]);

  return {
    checks: state.checks,
    /** A check is due or running for what the review shows now. */
    isChecking: signature != null && signature !== state.signature,
    error: state.error,
    /** Forget the checks (another file). */
    reset: () => {
      request.current++;
      inFlight.current = null;
      setState(EMPTY_STATE);
    },
  };
}
