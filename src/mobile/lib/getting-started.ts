import type { MenuPreferences, OnboardingProgress } from '@shared/schema';
import {
  gettingStartedItems,
  type GettingStartedItem,
  type GettingStartedKey,
} from '@/utils/getting-started';
import { routes, settingsPath } from '@/mobile/shell/routes';

/** Finance with the add-account form open (`?add=account`): the empty Přehled's first action. */
export const addAccountPath = `${routes.finance}?add=account`;

/** The query that opens the add-account form on Finance. */
export function opensAddAccount(search: string): boolean {
  return new URLSearchParams(search).get('add') === 'account';
}

/**
 * The desktop's "Začínáme" checklist (`gettingStartedItems`: the same items, the same done rules,
 * only the areas the person tracks) with the phone's pages as targets: the statement on Finance
 * (where accounts are added and imported into), an investment on Akcie (else Krypto), a budget on
 * Rozpočty and the backup on Nastavení → Záloha a přenos.
 */
export function phoneGettingStartedItems(
  progress: Pick<
    OnboardingProgress,
    'hasTransactions' | 'hasInvestment' | 'hasBudget' | 'lastBackupAt'
  >,
  prefs: MenuPreferences | undefined
): GettingStartedItem[] {
  return gettingStartedItems(progress, prefs).map((item) => ({
    ...item,
    href: phoneHref(item.key, item.href),
  }));
}

function phoneHref(key: GettingStartedKey, desktopHref: string): string {
  switch (key) {
    case 'importStatement':
      return routes.finance;
    case 'addInvestment':
      // `investmentPage` chose between the two by what is tracked
      return desktopHref === '/crypto' ? routes.cryptoList : routes.stocks;
    case 'setBudget':
      return routes.budgets;
    case 'makeBackup':
      return settingsPath('backup');
  }
}
