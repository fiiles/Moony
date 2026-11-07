import { useCallback, useSyncExternalStore } from 'react';
import { readSetting, writeSetting } from '@/utils/device-storage';
import { utcDayFloor } from '@/utils/chart-axis';
import {
  parseDismissals,
  pruneDismissals,
  type UpkeepDismissals,
} from '@/mobile/lib/upkeep-dismissals';

/** The device's entry: the phone-only Údržba rows put away here (they never show on the desktop). */
const STORAGE_KEY = 'moony-upkeep-dismissals';

const listeners = new Set<() => void>();
let cached: { text: string | null; value: UpkeepDismissals } | null = null;

function read(): UpkeepDismissals {
  const text = readSetting(STORAGE_KEY);
  if (cached === null || cached.text !== text) cached = { text, value: parseDismissals(text) };
  return cached.value;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * The Údržba rows put away on this device (B4: "Je čas na výpis z …" and "Nezařazené", which have
 * no desktop milestone to hide), and the setter that stores a new map and tells every reader. The
 * map is read from device storage; a storage that cannot be read is an empty map.
 */
export function useUpkeepDismissals(): {
  dismissals: UpkeepDismissals;
  setDismissals: (update: (current: UpkeepDismissals) => UpkeepDismissals) => void;
} {
  const dismissals = useSyncExternalStore(subscribe, read, read);
  const setDismissals = useCallback((update: (current: UpkeepDismissals) => UpkeepDismissals) => {
    // Snoozes that ran out go on every write, so the entry stays small
    const next = pruneDismissals(update(read()), utcDayFloor(Date.now() / 1000));
    writeSetting(STORAGE_KEY, JSON.stringify(next));
    for (const listener of listeners) listener();
  }, []);
  return { dismissals, setDismissals };
}
