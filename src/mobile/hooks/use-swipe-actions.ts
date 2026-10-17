import { useEffect, useRef, useState, type MouseEvent, type PointerEvent } from 'react';
import { swipeAction, swipeOffset } from '@/mobile/lib/swipe';

/** A finger that moved this far is a swipe or a scroll, not a tap or a long press. */
const SLOP_PX = 8;
/** Holding this long without moving is a long press (the context menu). */
const LONG_PRESS_MS = 500;

interface SwipeOptions {
  /** Swipe towards the left edge (the panel on the right). */
  onSwipeLeft?: () => void;
  /** Swipe towards the right edge (the panel on the left). */
  onSwipeRight?: () => void;
  onLongPress?: () => void;
}

interface Gesture {
  id: number;
  x: number;
  y: number;
  /** Past the slop: no longer a tap or a long press. */
  moved: boolean;
  /** The row follows the finger sideways. */
  horizontal: boolean;
}

/**
 * Swipe actions and the long press of a phone row (spec §10.3, prototype note 5), with pointer
 * events and no dependency. The row keeps `touch-action: pan-y`, so a vertical drag scrolls the
 * page (the browser cancels the gesture) and only a sideways drag moves the row: it follows the
 * finger up to the panel width, fires its action when released past the trigger and snaps back.
 * A tap that turned into a swipe or a long press never reaches the row's click.
 */
export function useSwipeActions({ onSwipeLeft, onSwipeRight, onLongPress }: SwipeOptions) {
  const [offset, setOffset] = useState(0);
  const [dragging, setDragging] = useState(false);
  const gesture = useRef<Gesture | null>(null);
  const offsetRef = useRef(0);
  const timer = useRef<number | null>(null);
  const suppressClick = useRef(false);
  const sides = { left: onSwipeLeft !== undefined, right: onSwipeRight !== undefined };

  const clearTimer = () => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
  };
  useEffect(() => clearTimer, []);

  const moveTo = (next: number) => {
    offsetRef.current = next;
    setOffset(next);
  };

  const longPress = () => {
    clearTimer();
    gesture.current = null;
    suppressClick.current = true;
    onLongPress?.();
  };

  const finish = (release: boolean) => {
    const g = gesture.current;
    gesture.current = null;
    clearTimer();
    if (!g) return;
    if (g.moved) suppressClick.current = true;
    if (!g.horizontal) return;
    const action = release ? swipeAction(offsetRef.current) : null;
    setDragging(false);
    moveTo(0);
    if (action === 'left') onSwipeLeft?.();
    else if (action === 'right') onSwipeRight?.();
  };

  const onPointerDown = (event: PointerEvent<HTMLElement>) => {
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    suppressClick.current = false;
    gesture.current = {
      id: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      moved: false,
      horizontal: false,
    };
    if (onLongPress) {
      clearTimer();
      timer.current = window.setTimeout(longPress, LONG_PRESS_MS);
    }
  };

  const onPointerMove = (event: PointerEvent<HTMLElement>) => {
    const g = gesture.current;
    if (!g || g.id !== event.pointerId) return;
    const dx = event.clientX - g.x;
    const dy = event.clientY - g.y;
    if (!g.moved) {
      if (Math.hypot(dx, dy) <= SLOP_PX) return;
      g.moved = true;
      clearTimer();
      g.horizontal = (sides.left || sides.right) && Math.abs(dx) > Math.abs(dy);
      if (g.horizontal) {
        event.currentTarget.setPointerCapture(event.pointerId);
        setDragging(true);
      }
    }
    if (g.horizontal) moveTo(swipeOffset(dx, sides));
  };

  return {
    /** Where the row sits (px, positive to the right). */
    offset,
    /** The finger holds the row: no snap transition. */
    dragging,
    bind: {
      onPointerDown,
      onPointerMove,
      onPointerUp: () => finish(true),
      onPointerCancel: () => finish(false),
      // Only the row's own capture: taking it over mid-gesture ends the touch's implicit capture
      // on the inner element under the finger, whose `lostpointercapture` bubbles up to here.
      onLostPointerCapture: (event: PointerEvent<HTMLElement>) => {
        if (event.target === event.currentTarget) finish(false);
      },
      onClickCapture: (event: MouseEvent<HTMLElement>) => {
        if (!suppressClick.current) return;
        suppressClick.current = false;
        event.preventDefault();
        event.stopPropagation();
      },
      // A right click, and the long press of Android Chrome, open the menu as the long press does.
      onContextMenu: (event: MouseEvent<HTMLElement>) => {
        if (!onLongPress) return;
        event.preventDefault();
        if (gesture.current?.moved) return;
        longPress();
      },
    },
  };
}
