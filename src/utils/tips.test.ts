import { describe, expect, it } from 'vitest';
import {
  MCP_CATEGORIZE_TIP_KEY,
  MCP_CATEGORIZE_TIP_LEGACY_KEY,
  MANUAL_CATEGORIZATION_KEY,
  MCP_INSURANCE_TIP_KEY,
  MCP_INSURANCE_TIP_LEGACY_KEY,
  dismissTip,
  hasCategorizedManually,
  isTipDismissed,
  markCategorizedManually,
} from './tips';

function fakeStorage(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
  };
}

describe('dismissible tips', () => {
  it('uses the documented localStorage keys', () => {
    expect(MCP_CATEGORIZE_TIP_KEY).toBe('moony-tip-mcp-categorize');
    expect(MCP_INSURANCE_TIP_KEY).toBe('moony-tip-mcp-insurance');
  });

  it('is shown until dismissed, and the dismissal is remembered', () => {
    const storage = fakeStorage();
    expect(isTipDismissed(MCP_CATEGORIZE_TIP_KEY, undefined, storage)).toBe(false);
    dismissTip(MCP_CATEGORIZE_TIP_KEY, storage);
    expect(storage.data.get(MCP_CATEGORIZE_TIP_KEY)).toBe('true');
    expect(isTipDismissed(MCP_CATEGORIZE_TIP_KEY, undefined, storage)).toBe(true);
  });

  it('honours dismissals stored under the previous banner keys', () => {
    const categorize = fakeStorage({ [MCP_CATEGORIZE_TIP_LEGACY_KEY]: 'true' });
    expect(isTipDismissed(MCP_CATEGORIZE_TIP_KEY, MCP_CATEGORIZE_TIP_LEGACY_KEY, categorize)).toBe(
      true
    );
    const insurance = fakeStorage({ [MCP_INSURANCE_TIP_LEGACY_KEY]: 'true' });
    expect(isTipDismissed(MCP_INSURANCE_TIP_KEY, MCP_INSURANCE_TIP_LEGACY_KEY, insurance)).toBe(
      true
    );
  });

  it('does not treat other values as a dismissal', () => {
    const storage = fakeStorage({ [MCP_CATEGORIZE_TIP_KEY]: 'false' });
    expect(isTipDismissed(MCP_CATEGORIZE_TIP_KEY, undefined, storage)).toBe(false);
  });

  it('falls back to "not dismissed" when storage is unavailable or throws', () => {
    const broken = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(isTipDismissed(MCP_CATEGORIZE_TIP_KEY, undefined, broken)).toBe(false);
    expect(() => dismissTip(MCP_CATEGORIZE_TIP_KEY, broken)).not.toThrow();
    expect(isTipDismissed(MCP_CATEGORIZE_TIP_KEY, undefined, undefined)).toBe(false);
  });
});

describe('manual categorization flag', () => {
  it('starts false and is set once the user categorizes a transaction by hand', () => {
    const storage = fakeStorage();
    expect(hasCategorizedManually(storage)).toBe(false);
    markCategorizedManually(storage);
    expect(storage.data.get(MANUAL_CATEGORIZATION_KEY)).toBe('true');
    expect(hasCategorizedManually(storage)).toBe(true);
  });
});
