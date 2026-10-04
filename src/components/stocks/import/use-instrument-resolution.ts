import { useCallback, useEffect, useRef, useState } from 'react';
import { stockImportApi } from '@/lib/tauri-api';
import type {
  StockImportInstrument,
  StockInstrumentQuery,
  StockInstrumentResolution,
} from '@shared/schema';
import { RESOLVE_CHUNK_SIZE, instrumentQuery, needsLookup } from './import-config';

export interface ResolutionProgress {
  done: number;
  total: number;
}

interface UseInstrumentResolutionOptions {
  /** Looking up starts when the review step is open, not before. */
  enabled: boolean;
  /** The instruments of the latest preview. */
  instruments: readonly StockImportInstrument[] | undefined;
  /** Called with every answer and the instruments it is about (to apply what was found). */
  onResolved: (
    instruments: readonly StockImportInstrument[],
    resolutions: readonly StockInstrumentResolution[]
  ) => void;
}

/** A lookup that did not happen: unverified, not unknown. */
function unanswered(key: string): StockInstrumentResolution {
  return { key, candidates: [], best: null, lookupFailed: true };
}

/**
 * Confirms the instruments of a file on Yahoo Finance (`resolveInstruments`):
 * once per instrument and file, in small chunks so progress can be shown, in
 * the background of the review. A lookup that fails (offline, rate limit) marks
 * the instrument unverified and never blocks anything; "skip" stops waiting.
 */
export function useInstrumentResolution({
  enabled,
  instruments,
  onResolved,
}: UseInstrumentResolutionOptions) {
  const [resolutions, setResolutions] = useState<Record<string, StockInstrumentResolution>>({});
  const [progress, setProgress] = useState<ResolutionProgress>({ done: 0, total: 0 });

  // Keys that were queued, and those that have an answer. Refs: they only decide
  // what to do next; what is shown comes from the state above.
  const asked = useRef(new Set<string>());
  const answered = useRef(new Set<string>());
  // Bumped to stop the lookups in flight (new file, "skip").
  const generation = useRef(0);
  const queue = useRef<Promise<void>>(Promise.resolve());
  const onResolvedRef = useRef(onResolved);
  useEffect(() => {
    onResolvedRef.current = onResolved;
  });

  const store = useCallback((answers: readonly StockInstrumentResolution[]) => {
    for (const answer of answers) answered.current.add(answer.key);
    setResolutions((current) => ({
      ...current,
      ...Object.fromEntries(answers.map((answer) => [answer.key, answer])),
    }));
    setProgress({ done: answered.current.size, total: asked.current.size });
  }, []);

  const lookUp = useCallback(
    (pending: readonly StockImportInstrument[]) => {
      const run = generation.current;
      for (const instrument of pending) asked.current.add(instrument.key);
      // One chain, so two batches of instruments never look up at the same time; it
      // must never end rejected, or nothing after it would run.
      queue.current = queue.current
        .then(async () => {
          setProgress({ done: answered.current.size, total: asked.current.size });
          for (let start = 0; start < pending.length; start += RESOLVE_CHUNK_SIZE) {
            if (run !== generation.current) return;
            const chunk = pending.slice(start, start + RESOLVE_CHUNK_SIZE);
            let answers: StockInstrumentResolution[];
            try {
              answers = await stockImportApi.resolveInstruments(chunk.map(instrumentQuery));
            } catch (error) {
              console.error('Instrument lookup failed:', error);
              answers = chunk.map((instrument) => unanswered(instrument.key));
            }
            if (run !== generation.current) return;
            // Whatever the backend left out is unverified, not pending forever.
            const returned = new Set(answers.map((answer) => answer.key));
            const complete = [
              ...answers,
              ...chunk.filter((i) => !returned.has(i.key)).map((i) => unanswered(i.key)),
            ];
            store(complete);
            onResolvedRef.current(chunk, complete);
          }
        })
        .catch((error: unknown) => {
          console.error('Instrument lookup stopped:', error);
        });
    },
    [store]
  );

  // Every instrument that is new or has no symbol is looked up once per file.
  useEffect(() => {
    if (!enabled || !instruments) return;
    const pending = instruments.filter((i) => needsLookup(i) && !asked.current.has(i.key));
    if (pending.length > 0) lookUp(pending);
  }, [enabled, instruments, lookUp]);

  /** Stop waiting: what has no answer yet stays unverified. */
  const skip = useCallback(() => {
    generation.current += 1;
    const open = [...asked.current].filter((key) => !answered.current.has(key));
    if (open.length > 0) store(open.map(unanswered));
  }, [store]);

  /** Forget everything: another file. */
  const reset = useCallback(() => {
    generation.current += 1;
    asked.current = new Set();
    answered.current = new Set();
    queue.current = Promise.resolve();
    setResolutions({});
    setProgress({ done: 0, total: 0 });
  }, []);

  /** An answer found without the backend (a listing picked from the search). */
  const remember = useCallback(
    (resolution: StockInstrumentResolution) => {
      asked.current.add(resolution.key);
      store([resolution]);
    },
    [store]
  );

  /** Confirm one symbol the user typed. */
  const verify = useCallback(
    async (query: StockInstrumentQuery) => {
      asked.current.add(query.key);
      try {
        const [answer] = await stockImportApi.resolveInstruments([query]);
        store([answer ?? unanswered(query.key)]);
      } catch (error) {
        console.error('Instrument lookup failed:', error);
        store([unanswered(query.key)]);
      }
    },
    [store]
  );

  return {
    resolutions,
    progress,
    isResolving: progress.done < progress.total,
    skip,
    reset,
    remember,
    verify,
  };
}
