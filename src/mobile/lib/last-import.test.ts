import { describe, expect, it } from 'vitest';
import { newestImportPerAccount } from './last-import';

const batch = (importedAt: number) => ({ importedAt });

describe('newestImportPerAccount', () => {
  it('is null for an account without batches', () => {
    expect(newestImportPerAccount(['a'], [[]])).toEqual({ a: null });
  });

  it('is null while an account has not answered yet', () => {
    expect(newestImportPerAccount(['a'], [undefined])).toEqual({ a: null });
  });

  it('is the newest importedAt, whatever the order of the batches', () => {
    expect(newestImportPerAccount(['a'], [[batch(100), batch(300), batch(200)]])).toEqual({
      a: 300,
    });
  });

  it('keeps the accounts apart', () => {
    expect(
      newestImportPerAccount(['a', 'b', 'c'], [[batch(500)], [], [batch(100), batch(900)]])
    ).toEqual({ a: 500, b: null, c: 900 });
  });

  it('is empty without accounts', () => {
    expect(newestImportPerAccount([], [])).toEqual({});
  });
});
