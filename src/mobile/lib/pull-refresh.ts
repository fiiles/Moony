/**
 * Pull to refresh (spec §8.8, audit C11): at the top of a screen, a downward drag pulls the content
 * down against a growing resistance, an indicator turns with it, and a release past the trigger
 * refreshes while the content waits a little lower; a release short of it settles back. The
 * platform feel: the finger's travel is damped (the content never runs ahead of it), the trigger
 * is felt as a tick once, and nothing happens on a screen that is scrolled down or on a drag that
 * leaves sideways (a row's swipe, the back swipe from the edge). Pure; the gesture is
 * `usePullToRefresh`. Distances in px, positive downwards.
 */

/**
 * Sent on `window` when a drag becomes a pull: a gesture that started on the same finger (the
 * hero chart's scrub) ends, since the pull now takes the touch moves and no pointercancel follows.
 */
export const PULL_START_EVENT = 'moony:pull-start';

/** Movement before the gesture decides whether it is a pull. */
export const PULL_SLOP_PX = 8;
/** The damped distance a release must reach to refresh. */
export const PULL_TRIGGER_PX = 64;
/** Where the content waits while the refresh runs. */
export const PULL_HOLD_PX = 52;
/** The damped distance never goes past this. */
export const PULL_MAX_PX = 120;
/** How far the finger travels for the damped distance to reach about two thirds of the maximum. */
const PULL_SCALE_PX = 200;

/** The damped distance for a finger travel of `dy` (0 for none or an upward travel). */
export function pullDistance(dy: number): number {
  if (dy <= 0) return 0;
  return PULL_MAX_PX * (1 - Math.exp(-dy / PULL_SCALE_PX));
}

/**
 * What a drag that started at the top is, once it has moved past the slop: `pull` when it heads
 * down more than sideways, `none` otherwise (the page scrolls, a row swipes); `null` while
 * undecided.
 */
export function pullDirection(dx: number, dy: number): 'pull' | 'none' | null {
  if (Math.hypot(dx, dy) < PULL_SLOP_PX) return null;
  return dy > 0 && dy > Math.abs(dx) * 1.2 ? 'pull' : 'none';
}

export type PullState =
  | { kind: 'idle' }
  /** A finger is down at the top; the drag has not decided yet. */
  | { kind: 'tracking'; x: number; y: number }
  /** The content follows the finger; `armed` once past the trigger. */
  | { kind: 'pulling'; x: number; y: number; distance: number; armed: boolean }
  | { kind: 'refreshing' }
  /** The drag went elsewhere (a scroll, a sideways swipe) until the finger lifts. */
  | { kind: 'ignored' };

export type PullEvent =
  /** A finger lands; `atTop` when the screen is scrolled to its top. */
  | { type: 'start'; x: number; y: number; atTop: boolean }
  | { type: 'move'; x: number; y: number }
  | { type: 'end' }
  /** The refresh has finished (or failed). */
  | { type: 'done' };

export const PULL_IDLE: PullState = { kind: 'idle' };

/**
 * The next state of the gesture. A refresh in progress ignores new touches until it is `done`; a
 * pull pushed back to or above its start is let go (`ignored`), so the same drag scrolls the
 * content; a release while armed refreshes.
 */
export function pullReducer(state: PullState, event: PullEvent): PullState {
  switch (event.type) {
    case 'start':
      if (state.kind === 'refreshing') return state;
      return event.atTop ? { kind: 'tracking', x: event.x, y: event.y } : { kind: 'ignored' };
    case 'move': {
      if (state.kind === 'tracking') {
        const direction = pullDirection(event.x - state.x, event.y - state.y);
        if (direction === null) return state;
        if (direction === 'none') return { kind: 'ignored' };
        return pulling(state.x, state.y, event.y - state.y);
      }
      if (state.kind === 'pulling') {
        const dy = event.y - state.y;
        return dy <= 0 ? { kind: 'ignored' } : pulling(state.x, state.y, dy);
      }
      return state;
    }
    case 'end':
      if (state.kind === 'pulling' && state.armed) return { kind: 'refreshing' };
      return state.kind === 'refreshing' ? state : PULL_IDLE;
    case 'done':
      return state.kind === 'refreshing' ? PULL_IDLE : state;
  }
}

function pulling(x: number, y: number, dy: number): PullState {
  const distance = pullDistance(dy);
  return { kind: 'pulling', x, y, distance, armed: distance >= PULL_TRIGGER_PX };
}

/**
 * Whether a move while the drag is still undecided must be kept from the WebView: a downward move
 * at the top would otherwise start WKWebView's own rubber band, which then owns the gesture. The
 * slop still tells a sideways drag apart; only the scroll is held back meanwhile.
 */
export function holdsUndecidedMove(state: PullState, x: number, y: number): boolean {
  // Only a mostly downward move: a sideways start stays with the WebView (a horizontal scroller)
  return state.kind === 'tracking' && y - state.y > Math.abs(x - state.x);
}

/** How far the content sits down in `state` (the indicator's room). */
export function pullOffset(state: PullState): number {
  if (state.kind === 'pulling') return state.distance;
  if (state.kind === 'refreshing') return PULL_HOLD_PX;
  return 0;
}

/** The indicator's progress towards the trigger, 0–1 (1 while refreshing). */
export function pullProgress(state: PullState): number {
  if (state.kind === 'refreshing') return 1;
  if (state.kind !== 'pulling') return 0;
  return Math.min(1, state.distance / PULL_TRIGGER_PX);
}
