/**
 * Items of the dashboard "Getting started" checklist. Only areas the user
 * chose (menu preferences, set in the onboarding wizard) get an item; the
 * account and the backup are the core steps, the rest are marked optional.
 */
import type { MenuPreferences, OnboardingProgress } from '@shared/schema';

export type GettingStartedKey =
  'importStatement' | 'addInvestment' | 'setBudget' | 'makeBackup' | 'protectData';

/** With the sync flag on, the last core item is "Ochránit data", done by a backup or by sync on (spec §7.3); without it today's "Udělat zálohu". */
export interface GettingStartedSync {
  featureOn: boolean;
  syncOn: boolean;
}

export interface GettingStartedItem {
  key: GettingStartedKey;
  done: boolean;
  href: string;
  optional: boolean;
}

type Progress = Pick<
  OnboardingProgress,
  'hasTransactions' | 'hasInvestment' | 'hasBudget' | 'lastBackupAt'
>;

/** A missing flag means "shown" (crypto and otherAssets default to true). */
const shown = (prefs: MenuPreferences | undefined, key: keyof MenuPreferences) =>
  prefs?.[key] !== false;

/** Where "add an investment" leads: stocks, else crypto; null when both pages are hidden. */
export function investmentPage(prefs: MenuPreferences | undefined): string | null {
  if (shown(prefs, 'investments')) return '/stocks';
  if (shown(prefs, 'crypto')) return '/crypto';
  return null;
}

export function gettingStartedItems(
  progress: Progress,
  prefs: MenuPreferences | undefined,
  sync: GettingStartedSync = { featureOn: false, syncOn: false }
): GettingStartedItem[] {
  const items: GettingStartedItem[] = [
    {
      key: 'importStatement',
      done: progress.hasTransactions,
      href: '/bank-accounts',
      optional: false,
    },
  ];
  // `hasInvestment` counts stocks and crypto, so the item needs one of those pages.
  const investmentHref = investmentPage(prefs);
  if (investmentHref) {
    items.push({
      key: 'addInvestment',
      done: progress.hasInvestment,
      href: investmentHref,
      optional: true,
    });
  }
  items.push(
    { key: 'setBudget', done: progress.hasBudget, href: '/reports/cashflow', optional: true },
    sync.featureOn
      ? {
          key: 'protectData',
          done: progress.lastBackupAt !== null || sync.syncOn,
          href: '/settings/data',
          optional: false,
        }
      : {
          key: 'makeBackup',
          done: progress.lastBackupAt !== null,
          href: '/settings/data',
          optional: false,
        }
  );
  return items;
}

/**
 * Whether the checklist item offers a second link, to the sync settings: only "Protect your data"
 * (its own link leads to the backup), while the flag is on, sync is off and the item is not done.
 */
export function offerSyncLink(
  item: Pick<GettingStartedItem, 'key' | 'done'>,
  sync: GettingStartedSync
): boolean {
  return item.key === 'protectData' && sync.featureOn && !sync.syncOn && !item.done;
}
