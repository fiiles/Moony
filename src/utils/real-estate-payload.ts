import type { InsertRealEstate, RealEstate } from '@shared/schema';

/**
 * Update payload for a property that changes only some of its fields.
 *
 * The backend `update_real_estate` replaces the whole property (rent, costs and notes are not
 * merged), so every field is sent back, with `overrides` applied. Empty notes are stored as
 * "no notes" rather than as an empty string.
 */
export function realEstateUpdatePayload(
  realEstate: RealEstate,
  overrides: Partial<InsertRealEstate> = {}
): InsertRealEstate {
  const notes =
    'notes' in overrides ? overrides.notes?.trim() || undefined : (realEstate.notes ?? undefined);
  return {
    name: realEstate.name,
    address: realEstate.address,
    type: realEstate.type,
    purchasePrice: realEstate.purchasePrice?.toString(),
    purchasePriceCurrency: realEstate.purchasePriceCurrency,
    marketPrice: realEstate.marketPrice?.toString(),
    marketPriceCurrency: realEstate.marketPriceCurrency,
    monthlyRent: realEstate.monthlyRent,
    monthlyRentCurrency: realEstate.monthlyRentCurrency ?? undefined,
    recurringCosts: realEstate.recurringCosts,
    photos: realEstate.photos,
    ...overrides,
    notes,
  };
}
