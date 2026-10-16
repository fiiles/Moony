import { describe, expect, it } from 'vitest';

// The frontend tsconfig has no Node typings, so `node:fs` and `node:vm` are imported through
// variable specifiers and typed by hand (same approach as src/index.test.ts).
type Fs = { readFileSync(url: URL, encoding: 'utf8'): string };
type Vm = { runInNewContext(code: string, context: object): unknown };
const fsSpecifier = 'node:fs';
const vmSpecifier = 'node:vm';
const { readFileSync } = (await import(/* @vite-ignore */ fsSpecifier)) as Fs;
const { runInNewContext } = (await import(/* @vite-ignore */ vmSpecifier)) as Vm;

const SRC = readFileSync(new URL('../../public/theme-boot.js', import.meta.url), 'utf8');

/** Runs the boot script in an isolated context that only offers what a page would. */
function boot(stored: string | null | (() => never), systemDark: boolean) {
  const attrs: Record<string, string> = {};
  const ctx = {
    localStorage: { getItem: typeof stored === 'function' ? stored : () => stored },
    matchMedia: (q: string) => ({ matches: q.includes('dark') && systemDark }),
    document: {
      documentElement: {
        setAttribute: (k: string, v: string) => {
          attrs[k] = v;
        },
      },
    },
  };
  runInNewContext(SRC, ctx);
  return attrs['data-theme'];
}

describe('theme boot script', () => {
  it('stamps dark for a stored dark', () => expect(boot('dark', false)).toBe('dark'));
  it('stamps light for a stored light even when the OS is dark', () =>
    expect(boot('light', true)).toBe('light'));
  it('follows the OS for system', () => {
    expect(boot('system', true)).toBe('dark');
    expect(boot('system', false)).toBe('light');
  });
  it('follows the OS for a missing or unknown key', () => {
    expect(boot(null, true)).toBe('dark');
    expect(boot('blue', false)).toBe('light');
  });
  it('stamps light and never throws when the storage throws', () =>
    expect(
      boot(() => {
        throw new Error('blocked');
      }, false)
    ).toBe('light'));
});
