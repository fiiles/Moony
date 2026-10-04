import type {
  StockImportInstrument,
  StockInstrumentQuery,
  StockInstrumentResolution,
} from '@shared/schema';
import { RESOLVE_CHUNK_SIZE, instrumentQuery } from './import-config';

export interface LookupProgress {
  /** Instruments with an answer. */
  done: number;
  /** Instruments asked about for this file. */
  total: number;
}

export interface LookupState {
  resolutions: Readonly<Record<string, StockInstrumentResolution>>;
  progress: LookupProgress;
}

export const EMPTY_LOOKUP_STATE: LookupState = { resolutions: {}, progress: { done: 0, total: 0 } };

export interface InstrumentLookupOptions {
  /** `resolveInstruments` of the backend: one answer per query, in any order. */
  resolve: (queries: StockInstrumentQuery[]) => Promise<StockInstrumentResolution[]>;
  /** Instruments looked up per call (the backend answers a whole call at once). */
  chunkSize?: number;
  /** The state changed (new answers, new progress). */
  onChange: (state: LookupState) => void;
  /** A chunk was answered: the instruments asked about and what came back, one answer each. */
  onResolved: (
    instruments: readonly StockImportInstrument[],
    resolutions: readonly StockInstrumentResolution[]
  ) => void;
}

/** A lookup that did not happen: unverified, not unknown. */
export function unanswered(key: string): StockInstrumentResolution {
  return { key, candidates: [], best: null, lookupFailed: true };
}

/**
 * The lookups of one file's instruments on Yahoo Finance, without React:
 * every instrument is asked about once, in small chunks one after the other so
 * progress can be shown and two batches never run at the same time. A call that
 * fails, or leaves an instrument out of its answer, makes it unverified;
 * `skip` stops waiting and `reset` forgets the file.
 */
export function createInstrumentLookup(options: InstrumentLookupOptions) {
  const chunkSize = options.chunkSize ?? RESOLVE_CHUNK_SIZE;

  // Bumped to stop the lookups in flight (another file, "skip").
  let generation = 0;
  let asked = new Set<string>();
  let answered = new Set<string>();
  let resolutions: Record<string, StockInstrumentResolution> = {};
  // One chain; it must never end rejected, or nothing queued after it would run.
  let queue: Promise<void> = Promise.resolve();

  const emit = () => {
    options.onChange({
      resolutions: { ...resolutions },
      progress: { done: answered.size, total: asked.size },
    });
  };

  const store = (answers: readonly StockInstrumentResolution[]) => {
    for (const answer of answers) {
      answered.add(answer.key);
      resolutions[answer.key] = answer;
    }
    emit();
  };

  /** Looks the instruments up once each: those already asked about are left alone. */
  const enqueue = (instruments: readonly StockImportInstrument[]) => {
    const pending = instruments.filter((instrument) => !asked.has(instrument.key));
    if (pending.length === 0) return;
    const run = generation;
    for (const instrument of pending) asked.add(instrument.key);
    emit();

    queue = queue
      .then(async () => {
        for (let start = 0; start < pending.length; start += chunkSize) {
          if (run !== generation) return;
          const chunk = pending.slice(start, start + chunkSize);
          let answers: StockInstrumentResolution[];
          try {
            answers = await options.resolve(chunk.map(instrumentQuery));
          } catch (error) {
            console.error('Instrument lookup failed:', error);
            answers = [];
          }
          if (run !== generation) return;
          // Whatever the backend left out is unverified, not pending forever.
          const returned = new Set(answers.map((answer) => answer.key));
          const complete = [
            ...answers.filter((answer) => chunk.some((i) => i.key === answer.key)),
            ...chunk.filter((i) => !returned.has(i.key)).map((i) => unanswered(i.key)),
          ];
          store(complete);
          options.onResolved(chunk, complete);
        }
      })
      .catch((error: unknown) => {
        console.error('Instrument lookup stopped:', error);
      });
  };

  /** Stop waiting: what has no answer yet stays unverified. */
  const skip = () => {
    generation += 1;
    // A call that hangs must not hold up what is asked for after this.
    queue = Promise.resolve();
    const open = [...asked].filter((key) => !answered.has(key));
    if (open.length > 0) store(open.map(unanswered));
  };

  /** Forget everything: another file. */
  const reset = () => {
    generation += 1;
    asked = new Set();
    answered = new Set();
    resolutions = {};
    queue = Promise.resolve();
    emit();
  };

  /** An answer found without the backend (a listing picked from the search). */
  const remember = (resolution: StockInstrumentResolution) => {
    asked.add(resolution.key);
    store([resolution]);
  };

  /** Confirm one symbol the user typed. */
  const verify = async (query: StockInstrumentQuery) => {
    asked.add(query.key);
    const run = generation;
    let answer: StockInstrumentResolution | undefined;
    try {
      [answer] = await options.resolve([query]);
    } catch (error) {
      console.error('Instrument lookup failed:', error);
    }
    if (run !== generation) return;
    store([answer?.key === query.key ? answer : unanswered(query.key)]);
  };

  return { enqueue, skip, reset, remember, verify, hasAsked: (key: string) => asked.has(key) };
}

export type InstrumentLookup = ReturnType<typeof createInstrumentLookup>;
