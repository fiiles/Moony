import type { OtherAsset } from '@shared/schema';
import { calculateAnnualYield, type YieldType } from '@shared/calculations';
import { convertToCzK, type CurrencyCode } from '@shared/currencies';

/** Derived numbers of one other asset, in CZK (the page converts to the display currency). */
export interface OtherAssetRow {
  asset: OtherAsset;
  quantity: number;
  /** Latest estimate per unit, native currency. */
  price: number;
  /** Purchase average per unit, native currency. */
  avgPurchase: number;
  valueCzk: number;
  costCzk: number;
  gainCzk: number;
  /** Gain relative to the purchase cost, 0–1 (0 when there is no cost). */
  gainPct: number;
  /** Yearly yield by the asset's yield type, CZK. */
  yieldCzk: number;
}

export function otherAssetRow(asset: OtherAsset): OtherAssetRow {
  const quantity = parseFloat(asset.quantity) || 0;
  const price = parseFloat(asset.marketPrice) || 0;
  const avgPurchase = parseFloat(asset.averagePurchasePrice) || 0;
  const currency = (asset.currency || 'CZK') as CurrencyCode;
  const valueCzk = convertToCzK(quantity * price, currency);
  const costCzk = convertToCzK(quantity * avgPurchase, currency);
  const yieldCzk = convertToCzK(
    calculateAnnualYield({
      yieldType: asset.yieldType as YieldType,
      yieldValue: parseFloat(asset.yieldValue || '0') || 0,
      quantity,
      averagePurchasePrice: avgPurchase,
      marketPrice: price,
    }),
    currency
  );
  return {
    asset,
    quantity,
    price,
    avgPurchase,
    valueCzk,
    costCzk,
    gainCzk: valueCzk - costCzk,
    gainPct: costCzk > 0 ? (valueCzk - costCzk) / costCzk : 0,
    yieldCzk,
  };
}
