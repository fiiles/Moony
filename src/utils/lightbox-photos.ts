/**
 * The photos a lightbox pages through are a snapshot taken when it opened. When the list is
 * refetched, drop the ids that no longer exist and keep the person on the same photo (or the next
 * one, or the last, when theirs is gone); the snapshot's order never changes.
 */
export interface LightboxState {
  ids: string[];
  index: number;
  close: boolean;
}

export function reconcileLightbox(
  ids: string[],
  liveIds: ReadonlySet<string>,
  index: number
): LightboxState {
  const kept = ids.filter((id) => liveIds.has(id));
  if (kept.length === 0) return { ids: [], index: 0, close: true };
  // Photos that stay before the current position: the current one (if it stays) sits right after.
  const before = ids.slice(0, index).filter((id) => liveIds.has(id)).length;
  return { ids: kept, index: Math.min(before, kept.length - 1), close: false };
}
