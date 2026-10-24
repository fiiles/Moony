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
const UTILITY = '(?:bg|text|border|fill|stroke|ring|shadow|from|via|to|divide|outline|decoration)';

/** What the gate rejects; each pattern names the kind of literal it catches. */
const FORBIDDEN: ReadonlyArray<readonly [string, RegExp]> = [
  ['a hex colour', /#[0-9a-f]{3,8}\b/i],
  ['an rgb() / rgba() colour', /\brgba?\(/i],
  ['an hsl() / hsla() colour', /\bhsla?\(/i],
  ['a Tailwind palette colour', new RegExp(`\\b${UTILITY}-${PALETTE}\\b`)],
];

/** A line (or the one above it) carrying this marker with a reason is exempt. */
const ESCAPE = 'colour-literal-ok:';

/** A comment-only line: prose may talk about a colour, it paints none. */
const isCommentLine = (line: string) => /^\s*(\/\/|\/\*|\*)/.test(line);

/** The literals found in `source`, as `line N: <kind> — <line>` entries. */
function findLiterals(source: string): string[] {
  const lines = source.split('\n');
  const found: string[] = [];
  lines.forEach((line, i) => {
    if (isCommentLine(line)) return;
    if (line.includes(ESCAPE) || (i > 0 && lines[i - 1].includes(ESCAPE))) return;
    for (const [kind, pattern] of FORBIDDEN) {
      if (pattern.test(line)) found.push(`line ${i + 1}: ${kind} — ${line.trim()}`);
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
    expect(findLiterals('<div className="bg-surface text-ink border-line" />')).toHaveLength(0);
    expect(findLiterals('// the old #fff literal is gone')).toHaveLength(0);
    expect(
      findLiterals('const c = "#fff"; // colour-literal-ok: a QR code is black on white')
    ).toHaveLength(0);
    expect(
      findLiterals('// colour-literal-ok: a QR code is black on white\nconst c = "#fff";')
    ).toHaveLength(0);
    expect(findLiterals('// colour-literal-ok: one\nconst a = 1;\nconst c = "#fff";')).toHaveLength(
      1
    );
  });

  it('has no hex, rgb(), hsl() or Tailwind palette colour in a non-test source', () => {
    const offenders = files.flatMap((file) =>
      findLiterals(readFileSync(new URL(file, ROOT), 'utf8')).map((hit) => `${file} ${hit}`)
    );
    expect(offenders, 'use a token (var(--…) or a themed class), or mark the line').toEqual([]);
  });

  it('reads chart colours through useChartTokens(), never a frozen useMemo(chartTokens)', () => {
    // `useMemo(chartTokens, [])` reads the palette once and keeps it: the chart stays in the theme
    // it mounted in after a switch. `useChartTokens()` re-reads on every theme change.
    const offenders = files.filter((file) =>
      /useMemo\(\s*chartTokens\b/.test(readFileSync(new URL(file, ROOT), 'utf8'))
    );
    expect(offenders).toEqual([]);
  });
});
