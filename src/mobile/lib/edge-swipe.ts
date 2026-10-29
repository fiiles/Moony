/**
 * Back by a swipe from the left edge of a pushed screen (the iOS gesture; the WKWebView does not
 * give it to a page): the screen follows the finger to the right and, released far enough or
 * flicked, goes back as its back button does. Pure; the gesture itself is `useEdgeSwipeBack`.
 * Distances in px, velocity in px per ms, positive to the right.
 */

/** A finger that lands at most this far from the left edge starts the gesture. */
export const EDGE_PX = 24;
/** Movement before the gesture decides whether it is a back swipe. */
const SLOP_PX = 10;
/** Released past this share of the screen width, the screen goes back. */
const COMMIT_SHARE = 1 / 3;
/** A flick at least this fast (to the right) goes back after a short travel. */
const FLICK_VELOCITY = 0.45;
/** The travel a flick needs. */
const FLICK_MIN_PX = 40;

/** True for a touch that lands on the left edge. */
export function edgeSwipeStarts(x: number): boolean {
  return x <= EDGE_PX;
}

/**
 * What a drag from the edge is, once it has moved past the slop: `back` when it heads right more
 * than it heads up or down, `none` otherwise (the page scrolls); `null` while it is undecided.
 */
export function edgeSwipeDirection(dx: number, dy: number): 'back' | 'none' | null {
  if (Math.hypot(dx, dy) < SLOP_PX) return null;
  return dx > 0 && dx > Math.abs(dy) * 1.2 ? 'back' : 'none';
}

/** True when a release at `dx` moving at `velocity` goes back rather than snapping home. */
export function edgeSwipeCommits(dx: number, velocity: number, width: number): boolean {
  if (velocity < 0) return false;
  if (dx >= width * COMMIT_SHARE) return true;
  return velocity >= FLICK_VELOCITY && dx >= FLICK_MIN_PX;
}
