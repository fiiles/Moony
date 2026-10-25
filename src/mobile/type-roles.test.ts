import { describe, expect, it } from 'vitest';

// The type-role gate of the phone shell (design-system README §13, plan 6a ruling on Dynamic Type):
// every size a phone screen sets is a `text-m-*` role, because only those follow the text size the
// person chose in iOS Settings (`--m-type-scale`). A px size, or a desktop role, stays put at "Largest".
// Fields are covered elsewhere: `src/index.css` floors every field of the shell at 16 px.
//
// The frontend tsconfig has no Node typings, so fs is imported through a variable specifier and
// typed by hand (the pattern of src/index.test.ts and no-colour-literals.test.ts).
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

/**
 * What the gate rejects: a kind, the class that is caught and, for a class that has a home, the
 * one file that may use it (the tab bar label and the swipe panel label do not scale, on purpose).
 */
const FORBIDDEN: ReadonlyArray<{ kind: string; pattern: RegExp; allowedIn?: string }> = [
  {
    kind: 'a fixed size (text-[13px], text-[0.8rem])',
    pattern: /\btext-\[\s*\d*\.?\d+(?:px|rem|em|pt)\s*\]/,
  },
  {
    kind: 'a Tailwind default size (text-sm); use a text-m-* role',
    pattern: /\btext-(?:xs|sm|base|lg|xl|[2-9]xl)\b/,
  },
  {
    kind: 'text-micro (the tab bar label only)',
    pattern: /\btext-micro\b/,
    allowedIn: 'shell/TabBar.tsx',
  },
  {
    kind: 'text-caption (the swipe panel label only; text-m-caption scales)',
    pattern: /\btext-caption\b/,
    allowedIn: 'components/EntityRow.tsx',
  },
];

/** A line (or the one above it) carrying this marker, followed by a reason, is exempt. */
const ESCAPE = /type-size-ok:\s*\S/;

/** A comment-only line: prose may name a class, it sets no size. */
const isCommentLine = (line: string) => /^\s*(\/\/|\/\*|\*)/.test(line);

/** The size classes `source` (the file `file`, relative to src/mobile/) sets, one entry per kind and line. */
function findSizes(source: string, file = ''): string[] {
  const lines = source.split('\n');
  const found: string[] = [];
  lines.forEach((line, i) => {
    if (isCommentLine(line)) return;
    if (ESCAPE.test(line) || (i > 0 && ESCAPE.test(lines[i - 1]))) return;
    for (const { kind, pattern, allowedIn } of FORBIDDEN) {
      if (allowedIn !== undefined && file === allowedIn) continue;
      if (pattern.test(line)) found.push(`line ${i + 1}: ${kind} — ${line.trim()}`);
    }
  });
  return found;
}

describe('the phone shell sets its text in text-m-* roles', () => {
  const files = sources(ROOT);

  it('finds the phone sources', () => {
    // A wrong path would make every other check vacuous.
    expect(files.length).toBeGreaterThan(100);
    expect(files).toContain('shell/TabBar.tsx');
    expect(files).toContain('components/EntityRow.tsx');
  });

  it('catches each kind of size and honours the home files and the escape marker', () => {
    expect(findSizes('<p className="text-[13px] font-semibold" />')).toHaveLength(1);
    expect(findSizes('<p className="md:text-[12.5px]" />')).toHaveLength(1);
    expect(findSizes('<p className="text-[0.8rem]" />')).toHaveLength(1);
    expect(findSizes('<p className="text-sm text-ink" />')).toHaveLength(1);
    expect(findSizes('<p className="text-2xl" />')).toHaveLength(1);
    expect(findSizes('<p className="text-micro" />', 'pages/Lock.tsx')).toHaveLength(1);
    expect(findSizes('<p className="text-caption" />', 'pages/Lock.tsx')).toHaveLength(1);
    // Where a fixed size lives on purpose.
    expect(findSizes('<span className="text-micro" />', 'shell/TabBar.tsx')).toHaveLength(0);
    expect(findSizes('<span className="text-caption" />', 'components/EntityRow.tsx')).toHaveLength(
      0
    );
    // Not the other file's home, and the home of one class is not the home of another.
    expect(findSizes('<p className="text-caption" />', 'shell/TabBar.tsx')).toHaveLength(1);
    expect(findSizes('<p className="text-[13px]" />', 'shell/TabBar.tsx')).toHaveLength(1);
    // The roles, the documented scaled size and the colours that look alike are fine.
    expect(findSizes('<p className="text-m-caption text-m-body text-ink-3" />')).toHaveLength(0);
    expect(findSizes('<p className="text-[length:calc(14px*var(--m-text-k))]" />')).toHaveLength(0);
    expect(findSizes('<p className="text-[#fff] text-[var(--x)]" />')).toHaveLength(0);
    expect(findSizes('// the old text-[13px] is gone')).toHaveLength(0);
    // The escape marker needs a reason, on the line or the one above.
    expect(
      findSizes('<p className="text-[13px]" /> // type-size-ok: a fixed hairline label')
    ).toHaveLength(0);
    expect(
      findSizes('// type-size-ok: a fixed hairline label\n<p className="text-[13px]" />')
    ).toHaveLength(0);
    expect(findSizes('<p className="text-[13px]" /> // type-size-ok:')).toHaveLength(1);
    expect(
      findSizes('// type-size-ok: one\nconst a = 1;\n<p className="text-[13px]" />')
    ).toHaveLength(1);
  });

  it('has no px size, desktop size or fixed label outside its home file', () => {
    const offenders = files.flatMap((file) =>
      findSizes(readFileSync(new URL(file, ROOT), 'utf8'), file).map((hit) => `${file} ${hit}`)
    );
    expect(
      offenders,
      'use a text-m-* role (or text-[length:calc(…*var(--m-text-k))] and say why), or mark the line'
    ).toEqual([]);
  });
});

// The shared desktop pieces that set a fixed size (EmptyState, ChartLegend, the report quality notes)
// reach a phone screen only through the wrappers in components/Scaled.tsx, which hand them `text-m-*`
// roles through the pieces' own class props. A direct import would bring the desktop size back.
const DESKTOP_PIECES = [
  '@/components/common/EmptyState',
  '@/components/charts/ChartLegend',
  '@/components/reports/ReportQualityNote',
];
const SCALED_HOME = 'components/Scaled.tsx';

/** The desktop piece modules `source` imports, once each (a static or re-export `from '…'`, on whichever line, or `import('…')`). */
function desktopPieceImports(source: string): string[] {
  const found: string[] = [];
  const pattern = /(?:\bfrom\s*|\bimport\s*\(\s*)['"]([^'"]+)['"]/g;
  for (const match of source.matchAll(pattern)) {
    if (DESKTOP_PIECES.includes(match[1]) && !found.includes(match[1])) found.push(match[1]);
  }
  return found;
}

describe('the phone imports the shared desktop pieces through the Scaled wrappers', () => {
  const files = sources(ROOT);

  it('finds the desktop pieces an import names, on one line or two', () => {
    expect(
      desktopPieceImports("import { ChartLegend } from '@/components/charts/ChartLegend';")
    ).toEqual(['@/components/charts/ChartLegend']);
    expect(
      desktopPieceImports(
        "import {\n  ChartLegend,\n  type LegendItem,\n} from '@/components/charts/ChartLegend';"
      )
    ).toEqual(['@/components/charts/ChartLegend']);
    expect(
      desktopPieceImports('const m = await import("@/components/common/EmptyState");')
    ).toEqual(['@/components/common/EmptyState']);
    // Other desktop modules, the wrappers and a type re-export are not the pieces.
    expect(desktopPieceImports("import { Card } from '@/components/ui/card';")).toEqual([]);
    expect(
      desktopPieceImports("import { ScaledEmptyState } from '@/mobile/components/Scaled';")
    ).toEqual([]);
    expect(
      desktopPieceImports("import type { LegendItem } from '@/mobile/components/Scaled';")
    ).toEqual([]);
  });

  it('imports all three desktop pieces in the one file that wraps them', () => {
    // Without this the allowance below would pass for a wrapper file that wraps nothing.
    expect(files).toContain(SCALED_HOME);
    expect(desktopPieceImports(readFileSync(new URL(SCALED_HOME, ROOT), 'utf8')).sort()).toEqual(
      [...DESKTOP_PIECES].sort()
    );
  });

  it('has no phone file importing a desktop piece directly', () => {
    const offenders = files
      .filter((file) => file !== SCALED_HOME)
      .flatMap((file) =>
        desktopPieceImports(readFileSync(new URL(file, ROOT), 'utf8')).map(
          (module) => `${file} imports ${module}`
        )
      );
    expect(offenders, 'import the phone wrapper from @/mobile/components/Scaled').toEqual([]);
  });
});
