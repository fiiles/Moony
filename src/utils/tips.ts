/**
 * Dismissible one-line tips. Whether a tip was dismissed, and whether the user has
 * already categorized a transaction by hand, are per-device conveniences kept in `localStorage`
 * through the safe helpers in `device-storage.ts`.
 */
import {
  deviceStorage,
  readSetting,
  writeSetting,
  type ReadableStorage,
  type WritableStorage,
} from './device-storage';

/** "An AI assistant can categorize the rest" tip on a bank account's transaction list. */
export const MCP_CATEGORIZE_TIP_KEY = 'moony-tip-mcp-categorize';
/** "Add policies via an AI assistant" tip in the insurance empty state. */
export const MCP_INSURANCE_TIP_KEY = 'moony-tip-mcp-insurance';

/** Keys of the banners these tips replaced: a user who dismissed one never sees the tip. */
export const MCP_CATEGORIZE_TIP_LEGACY_KEY = 'moony_categorization_mcp_hint_dismissed';
export const MCP_INSURANCE_TIP_LEGACY_KEY = 'moony_insurance_mcp_hint_dismissed';

/** Set once the user has picked a category for a transaction themselves (not by rule or AI). */
export const MANUAL_CATEGORIZATION_KEY = 'moony-categorized-manually';

export function isTipDismissed(
  key: string,
  legacyKey?: string,
  storage: ReadableStorage | undefined = deviceStorage()
): boolean {
  return (
    readSetting(key, storage) === 'true' ||
    (legacyKey !== undefined && readSetting(legacyKey, storage) === 'true')
  );
}

export function dismissTip(
  key: string,
  storage: WritableStorage | undefined = deviceStorage()
): void {
  writeSetting(key, 'true', storage);
}

export function hasCategorizedManually(
  storage: ReadableStorage | undefined = deviceStorage()
): boolean {
  return readSetting(MANUAL_CATEGORIZATION_KEY, storage) === 'true';
}

export function markCategorizedManually(
  storage: WritableStorage | undefined = deviceStorage()
): void {
  writeSetting(MANUAL_CATEGORIZATION_KEY, 'true', storage);
}
