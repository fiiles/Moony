import { useLayoutEffect, useRef, useState, type RefObject } from 'react';
import { fitSizeFor, settleFit } from '@/mobile/lib/fit-size';

/**
 * The floor of a fitted hero, amount or stat figure in `clamp(floor, fit, max)`: the title size at
 * the default text size (20 px), a fixed size. A floor that grew with the text size would sit above
 * the fit and cut (end in an ellipsis) a figure that fits at the default size.
 */
export const HERO_FLOOR = '20px';

/** Width of the room inside a box, fractional (`clientWidth` is rounded to whole pixels). */
function contentWidth(box: HTMLElement): number {
  const style = getComputedStyle(box);
  const edges =
    parseFloat(style.paddingLeft) +
    parseFloat(style.paddingRight) +
    parseFloat(style.borderLeftWidth) +
    parseFloat(style.borderRightWidth);
  return box.getBoundingClientRect().width - (Number.isFinite(edges) ? edges : 0);
}

/**
 * Shrink-to-fit for one-line figures (a money value must never wrap or end in an ellipsis): the
 * font size in px at which the widest `[data-fit-text]` element inside `container` just holds its
 * first child (an inline span with the text), or `null` while unmeasured. Text width scales with
 * the font size, so a measurement at whatever size is drawn now gives the same answer; the caller
 * clamps the result between its type tokens (`clamp(min, fit, max)`, the minimum a fixed px size,
 * never one that grows with the text size: a floor above the fit is a cut), and every element
 * measured shares the one size (`fitSizeFor`).
 *
 * Measures again when the container or a figure changes size (a figure's box changes height with
 * its font size, so a change of the text size, or a fit that did not hold exactly, is seen and
 * corrected from the size actually drawn), when the fonts have loaded, and when `watch` changes
 * (the text). A measure that only grows back one half-pixel step to the size just left is ignored
 * (`settleFit`), so two neighbouring sizes never take turns.
 */
export function useFitText(
  container: RefObject<HTMLElement | null>,
  watch: unknown
): number | null {
  const [fit, setFit] = useState<number | null>(null);
  /** The fit drawn now and the one before it, for `settleFit`. */
  const drawn = useRef<{ current: number | null; previous: number | null }>({
    current: null,
    previous: null,
  });

  useLayoutEffect(() => {
    const root = container.current;
    if (!root) return;
    const measure = () => {
      let size = Infinity;
      for (const box of root.querySelectorAll<HTMLElement>('[data-fit-text]')) {
        const text = box.firstElementChild;
        if (!(text instanceof HTMLElement)) continue;
        const own = fitSizeFor(
          contentWidth(box),
          text.getBoundingClientRect().width,
          parseFloat(getComputedStyle(box).fontSize)
        );
        if (own !== null) size = Math.min(size, own);
      }
      const next = settleFit(
        Number.isFinite(size) ? size : null,
        drawn.current.current,
        drawn.current.previous
      );
      if (next === drawn.current.current) return;
      drawn.current = { current: next, previous: drawn.current.current };
      setFit(next);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    for (const box of root.querySelectorAll('[data-fit-text]')) observer.observe(box);
    // `ready` is already settled when the fonts of this text start loading after the first
    // layout (WebKit loads the Latin Extended subset for "Kč" late): `loadingdone` catches that.
    const fonts = document.fonts;
    void fonts?.ready.then(measure);
    fonts?.addEventListener?.('loadingdone', measure);
    return () => {
      observer.disconnect();
      fonts?.removeEventListener?.('loadingdone', measure);
    };
  }, [container, watch]);

  return fit;
}
