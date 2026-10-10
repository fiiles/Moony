/** The module an automatic planning row comes from, by category key (design system README §11). */
export const PLAN_MODULES: Record<string, { labelKey: string; url: string }> = {
  personalLoans: { labelKey: 'nav.loans', url: '/loans' },
  investmentLoans: { labelKey: 'nav.loans', url: '/loans' },
  personalInsurance: { labelKey: 'nav.insurance', url: '/insurance' },
  investmentInsurance: { labelKey: 'nav.insurance', url: '/insurance' },
  personalRealEstateCosts: { labelKey: 'nav.realEstate', url: '/real-estate' },
  investmentRealEstateCosts: { labelKey: 'nav.realEstate', url: '/real-estate' },
  rentalIncome: { labelKey: 'nav.realEstate', url: '/real-estate' },
  stockDividends: { labelKey: 'nav.stocks', url: '/stocks' },
  bondsInterest: { labelKey: 'nav.bonds', url: '/bonds' },
  interestIncome: { labelKey: 'nav.bankAccounts', url: '/bank-accounts' },
};

export function planModule(categoryKey: string): { labelKey: string; url: string } | undefined {
  return PLAN_MODULES[categoryKey];
}
