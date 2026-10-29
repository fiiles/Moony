/**
 * Closing a bottom sheet by dragging it down (the iOS sheet gesture): the sheet follows the finger
 * and, released far enough or flicked, slides away; otherwise it settles back. Pure; the gesture
 * itself is `useSheetDrag`. Distances in px, velocity in px per ms, positive downwards.
 */

/** Movement before the gesture decides whether it drags the sheet. */
const SLOP_PX = 8;
/** Released past this share of the sheet height, the sheet closes. */
const COMMIT_SHARE = 1 / 4;
/** A flick at least this fast (downwards) closes after a short travel. */
const FLICK_VELOCITY = 0.5;
/** The travel a flick needs. */
const FLICK_MIN_PX = 32;
/** How far the sheet gives when pulled up past where it rests. */
const OVERPULL_PX = 24;

/**
 * What a drag on the sheet is, once it has moved past the slop: `close` when it heads down more
 * than sideways, `none` otherwise (the content keeps it); `null` while it is undecided.
 */
export function sheetDragDirection(dx: number, dy: number): 'close' | 'none' | null {
  if (Math.hypot(dx, dy) < SLOP_PX) return null;
  return dy > 0 && dy > Math.abs(dx) * 1.2 ? 'close' : 'none';
}

/** Where the sheet sits for a finger `dy` below where the drag started: up only a little, with resistance. */
export function sheetDragOffset(dy: number): number {
  if (dy >= 0) return dy;
  return -OVERPULL_PX * (1 - Math.exp(dy / (OVERPULL_PX * 3)));
}

/** True when a release at `dy` moving at `velocity` closes the sheet rather than settling it. */
export function sheetDragCommits(dy: number, velocity: number, height: number): boolean {
  if (velocity < 0) return false;
  if (dy > height * COMMIT_SHARE) return true;
  return velocity >= FLICK_VELOCITY && dy >= FLICK_MIN_PX;
}
