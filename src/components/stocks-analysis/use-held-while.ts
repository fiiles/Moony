import { useState } from 'react';

/**
 * While the next response loads, the page keeps showing the previous one. What the card derives
 * from the page's new state (the parts of a new selection, say) has no figures in that old
 * response yet and would blank rows for a moment, so it is held at its last settled value while
 * `hold` is true. `live` must keep its identity while it does not change (memoize it).
 */
export function useHeldWhile<T>(live: T, hold: boolean): T {
  const [held, setHeld] = useState(live);
  if (!hold && held !== live) setHeld(live);
  return hold ? held : live;
}
