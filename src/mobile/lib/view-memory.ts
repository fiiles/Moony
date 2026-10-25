/**
 * What a screen remembers of how it was left — a sort, a toggle, a chart period — for as long as
 * the app runs. A route remounts on every visit, so leaving Finance for an account and coming back
 * would otherwise reset it. In memory only: a relaunch starts fresh (nothing is written to storage).
 */
export interface ViewMemory {
  /** What was last set under `key`, or `initial` while nothing was. */
  get<T>(key: string, initial: T): T;
  set<T>(key: string, value: T): void;
}

export function createViewMemory(): ViewMemory {
  const values = new Map<string, unknown>();
  return {
    get: <T>(key: string, initial: T) => (values.has(key) ? (values.get(key) as T) : initial),
    set: (key, value) => void values.set(key, value),
  };
}

/** The app's one memory, shared by every screen (see `useRemembered`). */
export const viewMemory = createViewMemory();
