import { useRef, type PointerEvent } from 'react';

/**
 * A visible `···` inside a scrolling list opens on a tap, not on the touch: Radix opens a trigger
 * on `pointerdown`, so a scroll that starts on the `···` would open the menu. For a finger or a pen
 * the trigger's `pointerdown` is prevented (Radix then skips it) and the `click` toggles the menu —
 * a scroll cancels the click. A mouse and the keyboard keep Radix's own behaviour.
 */
export function useTapToOpen(open: boolean, onOpenChange: (open: boolean) => void) {
  const touched = useRef(false);
  return {
    onPointerDown: (event: PointerEvent<HTMLElement>) => {
      touched.current = event.pointerType !== 'mouse';
      if (touched.current) event.preventDefault();
    },
    // A touch that became a scroll has no click: forget it, or the next Enter would toggle twice
    onPointerCancel: () => {
      touched.current = false;
    },
    onClick: () => {
      if (!touched.current) return;
      touched.current = false;
      onOpenChange(!open);
    },
  };
}
