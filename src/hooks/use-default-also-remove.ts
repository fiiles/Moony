import { useEffect, useRef, type Dispatch, type SetStateAction } from 'react';
import type { RemovalPlan } from '@shared/schema';
import { defaultAlsoRemove } from '@/utils/sync-removal';

/**
 * The once-only step of `useDefaultAlsoRemove`: the first plan sets the ticks, every later one
 * leaves them alone (`pruneAlsoRemove` drops what a plan no longer offers).
 */
export function applyDefaultOnce(
  applied: { current: boolean },
  plan: RemovalPlan | undefined,
  setAlsoRemove: (ids: string[]) => void
) {
  if (!plan || applied.current) return;
  applied.current = true;
  setAlsoRemove(defaultAlsoRemove(plan));
}

/**
 * Starts the "Odebrat také" ticks with the plan's recently added devices, once per dialog opening.
 * The plan reloads on every tick, and a device the person unticked must stay unticked, so later
 * plans never set the ticks again (the ref lives as long as the dialog is mounted; they mount
 * only while open).
 */
export function useDefaultAlsoRemove(
  plan: RemovalPlan | undefined,
  setAlsoRemove: Dispatch<SetStateAction<string[]>>
) {
  const applied = useRef(false);
  useEffect(() => applyDefaultOnce(applied, plan, setAlsoRemove), [plan, setAlsoRemove]);
}
