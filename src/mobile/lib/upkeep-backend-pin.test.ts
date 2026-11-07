import { describe, expect, it } from 'vitest';
import { STALE_BALANCE_DAYS } from './upkeep';

// The phone lists the stale accounts under the backend's `balances_stale` milestone by the
// backend's own rule, so its threshold must stay the Rust constant. The milestone carries only a
// count, not the accounts, which is why the number lives twice; this test reads the Rust source
// and fails the moment the two part.
//
// The file sits outside src/ and the frontend tsconfig has no Node typings, so fs is imported
// through a variable specifier and typed by hand (the pattern of src/index.test.ts).
type Fs = { readFileSync(url: URL, encoding: 'utf8'): string };
const fsSpecifier = 'node:fs';
const { readFileSync } = (await import(/* @vite-ignore */ fsSpecifier)) as Fs;

const ACCOUNTS_RS = new URL(
  '../../../src-tauri/src/services/milestones/accounts.rs',
  import.meta.url
);

describe('STALE_BALANCE_DAYS', () => {
  it('is the backend milestone threshold', () => {
    const source = readFileSync(ACCOUNTS_RS, 'utf8');
    const match = /const STALE_BALANCE_DAYS: i64 = (\d+);/.exec(source);
    expect(match, 'the Rust constant').not.toBeNull();
    expect(STALE_BALANCE_DAYS).toBe(Number(match?.[1]));
  });
});
