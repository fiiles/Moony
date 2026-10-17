/**
 * Hide amounts on the phone (spec §4 principle 7, D6): one eye on Přehled covers every amount in
 * the app. The state is global and persists; "hide on launch" (a Settings switch from Phase 5,
 * read here already) starts every launch hidden. The pure parts: the stored state and the mask.
 */
import type { MoneyOptions } from '@/lib/format';

/** localStorage: "1" hidden, "0" shown. */
export const HIDE_AMOUNTS_KEY = 'moony-hide-amounts';
/** localStorage: "1" starts every launch hidden. */
export const HIDE_AMOUNTS_ON_LAUNCH_KEY = 'moony-hide-amounts-on-launch';

/** What stands in for a number. */
export const MASK = '••••';

function readFlag(storage: Pick<Storage, 'getItem'> | null | undefined, key: string): boolean {
  try {
    return storage?.getItem(key) === '1';
  } catch {
    return false;
  }
}

/**
 * The state a launch starts with: hidden when it was left hidden, or when "hide on launch" is on
 * (whatever was left). Read once, when the provider mounts.
 */
export function readHideAmounts(storage: Pick<Storage, 'getItem'> | null | undefined): boolean {
  return readFlag(storage, HIDE_AMOUNTS_ON_LAUNCH_KEY) || readFlag(storage, HIDE_AMOUNTS_KEY);
}

export function writeHideAmounts(
  storage: Pick<Storage, 'setItem'> | null | undefined,
  hidden: boolean
): void {
  try {
    storage?.setItem(HIDE_AMOUNTS_KEY, hidden ? '1' : '0');
  } catch {
    // Storage that cannot be written: the choice lasts for this session only.
  }
}

/**
 * The options an amount is formatted with while it is hidden: never compact. "6,78 mil. Kč" masks
 * to "•••• mil. Kč", and the word still says millions; in full it is "•••• Kč".
 */
export function hiddenMoneyOptions(options: MoneyOptions | undefined): MoneyOptions {
  return { ...options, compact: false };
}

/**
 * A digit run with the group and decimal separators between its digits: "4 182 560", "1 249,50",
 * "1.249,50", "1,249.50", "1’240". Spaces are any Unicode space (no-break, narrow no-break, thin).
 */
const NUMBER = /\p{Nd}+(?:[\s.,'’]\p{Nd}+)*/gu;

/**
 * A formatted amount with its number replaced and everything else kept: "4 182 560 Kč" →
 * "•••• Kč", "+ 1 249,50 €" → "+ •••• €", "6,78 mil. Kč" → "•••• mil. Kč". One mask for every
 * number, so neither the size nor the digits show.
 */
export function maskMoney(formatted: string): string {
  return formatted.replace(NUMBER, MASK);
}
