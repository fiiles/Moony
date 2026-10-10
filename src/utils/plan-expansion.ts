/**
 * Which planning categories are expanded, remembered per browser. Storage is a
 * convenience only: every access is guarded, and the page works without it.
 */
export const PLAN_EXPANSION_STORAGE_KEY = 'moony.cashflowPlanning.expanded';

type StorageLike = Pick<Storage, 'getItem' | 'setItem'>;

export function readExpanded(storage: StorageLike | null | undefined): Set<string> {
  try {
    const raw = storage?.getItem(PLAN_EXPANSION_STORAGE_KEY);
    if (!raw) return new Set();
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed)
      ? new Set(parsed.filter((key): key is string => typeof key === 'string'))
      : new Set();
  } catch {
    return new Set();
  }
}

export function writeExpanded(storage: StorageLike | null | undefined, keys: Set<string>): void {
  try {
    storage?.setItem(PLAN_EXPANSION_STORAGE_KEY, JSON.stringify([...keys]));
  } catch {
    // Private window or blocked storage: the page simply forgets on reload.
  }
}

export function toggleKey(keys: Set<string>, key: string): Set<string> {
  const next = new Set(keys);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  return next;
}

/** Expand or collapse every category of one section; other sections keep their state. */
export function setKeys(keys: Set<string>, categoryKeys: string[], expanded: boolean): Set<string> {
  const next = new Set(keys);
  for (const key of categoryKeys) {
    if (expanded) next.add(key);
    else next.delete(key);
  }
  return next;
}

export function allExpanded(keys: Set<string>, categoryKeys: string[]): boolean {
  return categoryKeys.length > 0 && categoryKeys.every((key) => keys.has(key));
}
