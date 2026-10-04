/**
 * Design tokens for code that needs a concrete color string — Recharts
 * attributes, canvas, inline SVG gradients. Values are read once from the
 * `:root` custom properties in src/index.css, so the single source of truth
 * stays the stylesheet. Light theme only; nothing here changes at runtime.
 */
const cache = new Map<string, string>();

/** `token('chart-line')` → "#34312b". Falls back to `var(--name)` outside a DOM. */
export function token(name: string): string {
  const cached = cache.get(name);
  if (cached) return cached;
  let value = '';
  if (typeof document !== 'undefined') {
    value = getComputedStyle(document.documentElement).getPropertyValue(`--${name}`).trim();
  }
  const resolved = value || `var(--${name})`;
  if (value) cache.set(name, resolved);
  return resolved;
}

/** The chart palette as plain strings (README §8). */
export function chartTokens() {
  return {
    line: token('chart-line'),
    area: token('chart-area'),
    areaEnd: token('paper-2'),
    grid: token('chart-grid'),
    cost: token('chart-cost'),
    axis: token('chart-axis'),
    crosshair: token('ink-4'),
    baseline: token('line'),
    paper: token('paper'),
    gain: token('gain'),
    loss: token('loss'),
    series: [token('s1'), token('s2'), token('s3'), token('s4')],
    other: token('s-other'),
    ink2: token('ink-2'),
  };
}

export type ChartTokens = ReturnType<typeof chartTokens>;
