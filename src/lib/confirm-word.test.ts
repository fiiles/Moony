import { describe, expect, it } from 'vitest';
import { confirmWordMatches } from './confirm-word';

describe('confirmWordMatches (audit B5)', () => {
  it('matches the translated word whatever its case and surrounding spaces', () => {
    expect(confirmWordMatches('smazat', 'SMAZAT')).toBe(true);
    expect(confirmWordMatches('  Smazat ', 'SMAZAT')).toBe(true);
    expect(confirmWordMatches('löschen', 'LÖSCHEN')).toBe(true);
    expect(confirmWordMatches('usuń', 'USUŃ')).toBe(true);
    expect(confirmWordMatches('削除', '削除')).toBe(true);
  });

  it('matches a word typed in another Unicode form (a decomposed ö)', () => {
    expect(confirmWordMatches('löschen', 'LÖSCHEN')).toBe(true);
  });

  it('refuses anything else, an empty word included', () => {
    expect(confirmWordMatches('smaz', 'SMAZAT')).toBe(false);
    expect(confirmWordMatches('DELETE', 'SMAZAT')).toBe(false);
    expect(confirmWordMatches('loschen', 'LÖSCHEN')).toBe(false);
    expect(confirmWordMatches('', '')).toBe(false);
  });
});
