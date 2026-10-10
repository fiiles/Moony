import { NAV_GROUPS, type NavItem } from '@/components/common/nav-config';

export interface PlanModule {
  labelKey: string;
  url: string;
}

/** Nav item id of the module an automatic planning row comes from, by category key (design system README §11). */
const MODULE_BY_CATEGORY: Record<string, string> = {
  personalLoans: 'loans',
  investmentLoans: 'loans',
  personalInsurance: 'insurance',
  investmentInsurance: 'insurance',
  personalRealEstateCosts: 'realEstate',
  investmentRealEstateCosts: 'realEstate',
  rentalIncome: 'realEstate',
  stockDividends: 'stocks',
  bondsInterest: 'bonds',
  interestIncome: 'bankAccounts',
};

const NAV_ITEMS_BY_ID = new Map<string, NavItem>(
  NAV_GROUPS.flatMap((group) => group.items).map((item) => [item.id, item])
);

/** Label key and route of the module behind a category, taken from the real navigation. */
export function planModule(categoryKey: string): PlanModule | undefined {
  const item = NAV_ITEMS_BY_ID.get(MODULE_BY_CATEGORY[categoryKey] ?? '');
  return item ? { labelKey: item.labelKey, url: item.url } : undefined;
}
