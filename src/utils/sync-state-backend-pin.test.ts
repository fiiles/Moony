import { describe, expect, it } from 'vitest';
import { SYNC_STALE_DAYS, TRANSIENT_PROBLEMS } from './sync-state';

// The backend raises the `sync_stalled` milestone by the same 7-day rule and the same set of
// "server out of reach" problems the indicator uses, so both must stay the Rust values. This test
// reads the Rust source and fails the moment they part (the pattern of
// src/mobile/lib/upkeep-backend-pin.test.ts; fs is imported through a variable specifier because
// the frontend tsconfig has no Node typings).
type Fs = { readFileSync(url: URL, encoding: 'utf8'): string };
const fsSpecifier = 'node:fs';
const { readFileSync } = (await import(/* @vite-ignore */ fsSpecifier)) as Fs;

const UPKEEP_RS = new URL('../../src-tauri/src/services/milestones/upkeep.rs', import.meta.url);

describe('sync-state constants', () => {
  const source = readFileSync(UPKEEP_RS, 'utf8');

  it('SYNC_STALE_DAYS is the backend threshold', () => {
    const match = /const SYNC_STALE_DAYS: i64 = (\d+);/.exec(source);
    expect(match, 'the Rust constant').not.toBeNull();
    expect(SYNC_STALE_DAYS).toBe(Number(match?.[1]));
  });

  it('TRANSIENT_PROBLEMS are the backend list, in order', () => {
    const block = /const TRANSIENT_PROBLEMS: \[SyncProblem; \d+\] = \[([^\]]*)\]/.exec(source);
    expect(block, 'the Rust list').not.toBeNull();
    const names = [...(block?.[1] ?? '').matchAll(/SyncProblem::(\w+)/g)].map(
      (m) => m[1].charAt(0).toLowerCase() + m[1].slice(1)
    );
    expect(names).toEqual([...TRANSIENT_PROBLEMS]);
  });
});
