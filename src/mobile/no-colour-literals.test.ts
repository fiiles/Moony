import { describe, expect, it } from 'vitest';

// The colour gate of the phone shell (design-system README §13, dark-mode spec D9): every colour a
// phone screen paints comes from the token block, because only the token block follows
// `data-theme`. A literal colour is the one thing a theme switch cannot reach.
//
// The frontend tsconfig has no Node typings, so fs is imported through a variable specifier and
// typed by hand (the pattern of src/index.test.ts).
type Dirent = { name: string; isDirectory(): boolean; isFile(): boolean };
type Fs = {
  readdirSync(url: URL, options: { withFileTypes: true }): Dirent[];
  readFileSync(url: URL, encoding: 'utf8'): string;
};
const fsSpecifier = 'node:fs';
const { readdirSync, readFileSync } = (await import(/* @vite-ignore */ fsSpecifier)) as Fs;

const ROOT = new URL('./', import.meta.url);

/** Every non-test `.ts` / `.tsx` file below `dir`, as a path relative to src/mobile/. */
function sources(dir: URL, prefix = ''): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      out.push(...sources(new URL(`${entry.name}/`, dir), `${prefix}${entry.name}/`));
    } else if (/\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) {
      out.push(`${prefix}${entry.name}`);
    }
  }
  return out.sort();
}

const PALETTE =
  '(?:white|black|slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)';
// `border-<side>-`, `ring-offset-` and the form-control utilities paint a colour as much as `bg-` does.
const UTILITY =
  '(?:bg|text|border|border-[trblxyse]|fill|stroke|ring|ring-offset|shadow|from|via|to|divide|outline|decoration|accent|caret|placeholder)';

/** What the gate rejects; each pattern names the kind of literal it catches. */
const FORBIDDEN: ReadonlyArray<readonly [string, RegExp]> = [
  ['a hex colour', /#[0-9a-f]{3,8}\b/i],
  ['an rgb() / rgba() colour', /\brgba?\(/i],
  ['an hsl() / hsla() colour', /\bhsla?\(/i],
  ['an hwb(), lab(), lch(), oklab() or oklch() colour', /\b(?:hwb|oklch|oklab|lch|lab)\(/i],
  ['a Tailwind palette colour', new RegExp(`\\b${UTILITY}-${PALETTE}\\b`)],
];

/** A line (or the one above it) carrying this marker, followed by a reason, is exempt. */
const ESCAPE = /colour-literal-ok:\s*\S/;

/** A comment-only line: prose may talk about a colour, it paints none. */
const isCommentLine = (line: string) => /^\s*(\/\/|\/\*|\*)/.test(line);

/** The literals found in `source`, as `line N: <kind> — <line>` entries. */
function findLiterals(source: string): string[] {
  const lines = source.split('\n');
  const found: string[] = [];
  lines.forEach((line, i) => {
    if (isCommentLine(line)) return;
    if (ESCAPE.test(line) || (i > 0 && ESCAPE.test(lines[i - 1]))) return;
    for (const [kind, pattern] of FORBIDDEN) {
      if (pattern.test(line)) found.push(`line ${i + 1}: ${kind} — ${line.trim()}`);
    }
  });
  return found;
}

/**
 * The lines of `source` that name `chartTokens`, the one-shot reader of the chart palette
 * (`src/lib/tokens.ts`): imported by a phone chart it reads the palette once and keeps it, so the
 * chart stays in the theme it mounted in. `useChartTokens()` re-reads it on every switch. A
 * comment may name it; code under src/mobile may not.
 */
function findChartTokens(source: string): string[] {
  const found: string[] = [];
  source.split('\n').forEach((line, i) => {
    if (!isCommentLine(line) && /\bchartTokens\b/.test(line)) {
      found.push(`line ${i + 1}: ${line.trim()}`);
    }
  });
  return found;
}

describe('the phone shell paints no literal colours', () => {
  const files = sources(ROOT);

  it('finds the phone sources', () => {
    // A wrong path would make every other check vacuous.
    expect(files.length).toBeGreaterThan(100);
    expect(files).toContain('MobileApp.tsx');
  });

  it('catches each kind of literal and honours the escape marker', () => {
    expect(findLiterals('const c = "#fff";')).toHaveLength(1);
    expect(findLiterals('const c = "#1A2B3C4D";')).toHaveLength(1);
    expect(findLiterals('const c = `rgba(0, 0, 0, .2)`;')).toHaveLength(1);
    expect(findLiterals('const c = "hsl(210 50% 40%)";')).toHaveLength(1);
    // One entry per kind and line, however many classes of that kind the line holds.
    expect(findLiterals('<div className="bg-white text-slate-500" />')).toHaveLength(1);
    expect(findLiterals('<path className="stroke-rose-500" />')).toHaveLength(1);
    expect(findLiterals('const c = "oklch(0.7 0.1 200)";')).toHaveLength(1);
    expect(findLiterals('const c = "hwb(200 10% 10%)";')).toHaveLength(1);
    expect(findLiterals('const c = "lab(50% 40 59.5)";')).toHaveLength(1);
    // The side, offset and form-control utilities of Tailwind carry a palette colour too.
    expect(findLiterals('<div className="border-t-red-500" />')).toHaveLength(1);
    expect(findLiterals('<div className="border-x-slate-200" />')).toHaveLength(1);
    expect(findLiterals('<div className="ring-offset-white" />')).toHaveLength(1);
    expect(findLiterals('<input className="accent-indigo-500" />')).toHaveLength(1);
    expect(findLiterals('<input className="caret-blue-500" />')).toHaveLength(1);
    expect(findLiterals('<input className="placeholder-gray-400" />')).toHaveLength(1);
    expect(findLiterals('<div className="outline-black" />')).toHaveLength(1);
    // The themed names that look alike are tokens, not palette colours.
    expect(
      findLiterals('<div className="border-t-line ring-offset-canvas caret-ink" />')
    ).toHaveLength(0);
    expect(
      findLiterals('<input className="placeholder:text-ink-4 border-line-strong" />')
    ).toHaveLength(0);
    expect(findLiterals('<div className="bg-surface text-ink border-line" />')).toHaveLength(0);
    expect(findLiterals('// the old #fff literal is gone')).toHaveLength(0);
    expect(
      findLiterals('const c = "#fff"; // colour-literal-ok: a QR code is black on white')
    ).toHaveLength(0);
    expect(
      findLiterals('// colour-literal-ok: a QR code is black on white\nconst c = "#fff";')
    ).toHaveLength(0);
    // A marker without a reason exempts nothing.
    expect(findLiterals('const c = "#fff"; // colour-literal-ok:')).toHaveLength(1);
    expect(findLiterals('const c = "#fff"; // colour-literal-ok:   ')).toHaveLength(1);
    expect(findLiterals('// colour-literal-ok:\nconst c = "#fff";')).toHaveLength(1);
    expect(findLiterals('// colour-literal-ok: one\nconst a = 1;\nconst c = "#fff";')).toHaveLength(
      1
    );
  });

  it('has no hex, rgb(), hsl() or other colour function, or Tailwind palette colour in a non-test source', () => {
    const offenders = files.flatMap((file) =>
      findLiterals(readFileSync(new URL(file, ROOT), 'utf8')).map((hit) => `${file} ${hit}`)
    );
    expect(offenders, 'use a token (var(--…) or a themed class), or mark the line').toEqual([]);
  });

  it('catches the one-shot chart palette and lets the comments talk about it', () => {
    expect(findChartTokens("import { chartTokens } from '@/lib/tokens';")).toHaveLength(1);
    expect(
      findChartTokens('import {\n  chartTokens,\n  token,\n} from "@/lib/tokens";')
    ).toHaveLength(1);
    expect(findChartTokens("import { chartTokens as palette } from '@/lib/tokens';")).toHaveLength(
      1
    );
    expect(findChartTokens('const tk = useMemo(chartTokens, []);')).toHaveLength(1);
    expect(findChartTokens("import { useChartTokens } from '@/theme/ThemeProvider';")).toHaveLength(
      0
    );
    expect(findChartTokens('// never chartTokens() here: it reads the palette once')).toHaveLength(
      0
    );
  });

  it('never imports chartTokens: charts read the palette through useChartTokens()', () => {
    // `chartTokens` reads the palette once and keeps it: the chart stays in the theme it mounted in
    // after a switch. `useChartTokens()` re-reads it on every theme change.
    const offenders = files.flatMap((file) =>
      findChartTokens(readFileSync(new URL(file, ROOT), 'utf8')).map((hit) => `${file} ${hit}`)
    );
    expect(offenders).toEqual([]);
  });
});
