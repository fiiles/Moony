/**
 * Swipe actions of a phone row (spec §10.3, prototype note 5): the row follows the finger up to
 * the width of its action panel and fires the action when released past the trigger, then snaps
 * back. Pure; the gesture itself is `useSwipeActions`. Offsets in px, positive to the right.
 */

/** Width of the action panel a row uncovers (prototype `.row__action`). */
export const SWIPE_MAX = 98;
/** Release past this offset fires the action. */
export const SWIPE_TRIGGER = 60;

/** Where the row sits for a finger `dx` from its start: clamped, and still towards a side without an action. */
export function swipeOffset(dx: number, sides: { left: boolean; right: boolean }): number {
  if (dx > 0) return sides.right ? Math.min(dx, SWIPE_MAX) : 0;
  if (dx < 0) return sides.left ? Math.max(dx, -SWIPE_MAX) : 0;
  return 0;
}

/** The action a release at `offset` fires: `right` swipes towards the right edge. */
export function swipeAction(offset: number): 'left' | 'right' | null {
  if (offset >= SWIPE_TRIGGER) return 'right';
  if (offset <= -SWIPE_TRIGGER) return 'left';
  return null;
}
