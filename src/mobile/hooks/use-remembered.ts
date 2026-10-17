import { useCallback, useState } from 'react';
import { viewMemory } from '@/mobile/lib/view-memory';

/**
 * `useState` that survives the screen unmounting: it starts from what was last set under `key`
 * (else `initial`) and writes every change back, so a route left and revisited shows what it
 * showed. For the app session only (`view-memory.ts`). The key is read once, on mount; give every
 * remembered value its own, as `finance.sort`.
 */
export function useRemembered<T>(key: string, initial: T): [T, (next: T) => void] {
  const [value, setValue] = useState<T>(() => viewMemory.get(key, initial));
  const set = useCallback(
    (next: T) => {
      viewMemory.set(key, next);
      setValue(next);
    },
    [key]
  );
  return [value, set];
}
