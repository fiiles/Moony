import { describe, expect, it } from 'vitest';

// Numeric weight classes (`font-600`) work only for the numbers `tailwind.config.ts` lists under
// `theme.extend.fontWeight`: Tailwind's own weights are named (`font-semibold`), so an unlisted
// number generates no CSS and the element silently keeps its text role's weight. Until 2026-11-17
// `font-500`, `font-600` and `font-700` were missing and did nothing in some 200 files.
//
// The frontend tsconfig has no Node typings, so fs is imported through a variable specifier and
// typed by hand (the pattern of src/index.test.ts and src/mobile/type-roles.test.ts).
type Dirent = { name: string; isDirectory(): boolean };
type Fs = {
  readdirSync(url: URL, options: { withFileTypes: true }): Dirent[];
  readFileSync(url: URL, encoding: 'utf8'): string;
};
const fsSpecifier = 'node:fs';
const { readdirSync, readFileSync } = (await import(/* @vite-ignore */ fsSpecifier)) as Fs;

const SRC = new URL('./', import.meta.url);
const CONFIG = new URL('../tailwind.config.ts', import.meta.url);

/** Every `.ts` / `.tsx` file below `dir`, as a path relative to src/. */
function sources(dir: URL, prefix = ''): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      out.push(...sources(new URL(`${entry.name}/`, dir), `${prefix}${entry.name}/`));
    } else if (/\.tsx?$/.test(entry.name)) {
      out.push(`${prefix}${entry.name}`);
    }
  }
  return out.sort();
}

/** The numeric keys of `theme.extend.fontWeight` in tailwind.config.ts. */
function configuredWeights(): Set<string> {
  const config = readFileSync(CONFIG, 'utf8');
  const block = /fontWeight:\s*\{([^}]*)\}/.exec(config);
  if (!block) throw new Error('tailwind.config.ts has no theme.extend.fontWeight block');
  return new Set([...block[1].matchAll(/^\s*(\d{3}):/gm)].map((m) => m[1]));
}

describe('numeric font weight classes', () => {
  it('reads the configured weights', () => {
    expect([...configuredWeights()]).toEqual(expect.arrayContaining(['500', '600', '650', '700']));
  });

  it('uses only weights that tailwind.config.ts defines', () => {
    const weights = configuredWeights();
    const offenders: string[] = [];
    for (const file of sources(SRC)) {
      if (file === 'font-weights.test.ts') continue;
      const lines = readFileSync(new URL(file, SRC), 'utf8').split('\n');
      lines.forEach((line, i) => {
        for (const m of line.matchAll(/(?<![\w-])font-(\d{3})(?![\w-])/g)) {
          if (!weights.has(m[1])) offenders.push(`src/${file}:${i + 1} font-${m[1]}`);
        }
      });
    }
    expect(offenders).toEqual([]);
  });
});
