import { describe, expect, it } from 'vitest';

// The frontend tsconfig has no Node typings and Vitest returns an empty string for a CSS `?raw`
// import, so the files are read with fs, imported through a variable specifier and typed by hand.
type Fs = { readFileSync(url: URL, encoding: 'utf8'): string };
const fsSpecifier = 'node:fs';
const { readFileSync } = (await import(/* @vite-ignore */ fsSpecifier)) as Fs;
/** Prettier rewrites `[data-theme="dark"]` to single quotes in src/; tokens.css keeps double. */
const read = (path: string) =>
  readFileSync(new URL(path, import.meta.url), 'utf8').replace(/'/g, '"');
const INDEX = read('./index.css');
const TOKENS = read('../docs/design-system/tokens.css');
/** Theme-independent names, plus the two gradients that follow `--dark-hi/lo` through var(). */
const THEME_FREE =
  /^--(r-|fs-|fw-|ls-|sp-|font$|dark-grad|sidebar-w$|page-|control-h|input-h$|nav-row-h$|table-row-h$|t-fast$|t-base$|ease$)/;

function block(css: string, selector: string): Map<string, string> {
  const start = css.indexOf(selector + ' {');
  expect(start, `${selector} block`).toBeGreaterThanOrEqual(0);
  const body = css.slice(start, css.indexOf('\n}', start));
  const out = new Map<string, string>();
  for (const m of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out.set(m[1], normalize(m[2]));
  return out;
}
function normalize(v: string) {
  return v
    .replace(/\s+/g, ' ')
    .replace(/\b0\.(\d*?)0+\b/g, '0.$1')
    .replace(/\b(\d)\.0\b/g, '$1')
    .trim();
}

describe('theme token blocks', () => {
  it('the dark block declares exactly the themed names of the light block', () => {
    const light = [...block(INDEX, ':root').keys()].filter((n) => !THEME_FREE.test(n));
    const dark = [...block(INDEX, ':root[data-theme="dark"]').keys()];
    expect(dark.sort()).toEqual(light.sort());
  });
  it('tokens.css carries the same declarations as index.css in both blocks', () => {
    for (const sel of [':root', ':root[data-theme="dark"]']) {
      const app = block(INDEX, sel);
      const doc = block(TOKENS, sel);
      for (const [name, value] of app) {
        expect(doc.get(name), `${sel} ${name}`).toBe(value);
      }
    }
  });
  it('the dark root switches native controls', () => {
    expect(block(INDEX, ':root[data-theme="dark"]').size).toBeGreaterThan(50);
    expect(INDEX).toMatch(/:root\[data-theme="dark"\] \{\s*color-scheme: dark;/);
  });
});
