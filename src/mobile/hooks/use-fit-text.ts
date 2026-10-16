import { useLayoutEffect, useState, type RefObject } from 'react';

/**
 * Shrink-to-fit for one-line figures (a money value must never wrap or end in an ellipsis): the
 * font size in px at which the widest `[data-fit-text]` element inside `container` just holds its
 * first child (an inline span with the text), or `null` while unmeasured. Text width scales with
 * the font size, so one measurement at the current size is enough; the caller clamps the result
 * between its type tokens (`clamp(min, fit, max)`), and every element measured shares the one
 * size. Re-measures when the container resizes, when the fonts have loaded and when `watch`
 * changes (the text). Half-pixel steps, so a sub-pixel resize does not re-render.
 */
export function useFitText(
  container: RefObject<HTMLElement | null>,
  watch: unknown
): number | null {
  const [fit, setFit] = useState<number | null>(null);

  useLayoutEffect(() => {
    const root = container.current;
    if (!root) return;
    const measure = () => {
      let size = Infinity;
      for (const box of root.querySelectorAll<HTMLElement>('[data-fit-text]')) {
        const text = box.firstElementChild;
        if (!(text instanceof HTMLElement)) continue;
        const current = parseFloat(getComputedStyle(box).fontSize);
        const natural = text.getBoundingClientRect().width;
        if (current > 0 && natural > 0 && box.clientWidth > 0) {
          size = Math.min(size, (box.clientWidth * current) / natural);
        }
      }
      setFit(Number.isFinite(size) ? Math.floor(size * 2) / 2 : null);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    void document.fonts?.ready.then(measure);
    return () => observer.disconnect();
  }, [container, watch]);

  return fit;
}
