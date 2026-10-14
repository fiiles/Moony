import type { OtherAsset } from '@shared/schema';
import { calculateAnnualYield, type YieldType } from '@shared/calculations';
import { MissingTracker, type ConvertAtFn, type ConvertFn } from '@shared/fx';
import { recordCurrency } from '@shared/currencies';

/** One buy of an other asset, in the transaction's own currency. */
export interface OtherAssetBuy {
  quantity: number;
  pricePerUnit: number;
  currency: string;
  /** Unix seconds. */
  date: number;
}

/**
 * Derived numbers of one other asset: native figures plus their main-currency values
 * (`null` when that value cannot be converted).
 */
export interface OtherAssetRow {
  asset: OtherAsset;
  /** The asset's currency, the purchase average's (legacy rows without one are CZK). */
  currency: string;
  /** The latest estimate's currency: its valuation's, which can differ from the asset's. */
  priceCurrency: string;
  /** The yield's currency: the price's for a yield on the market value, else the asset's. */
  yieldCurrency: string;
  quantity: number;
  /** Latest estimate per unit, in `priceCurrency`. */
  price: number;
  /** Purchase average per unit, in `currency`. */
  avgPurchase: number;
  /** quantity × price (`priceCurrency`), quantity × purchase average (`currency`) and the
   * yearly yield (`yieldCurrency`), native. */
  valueNative: number;
  costNative: number;
  yieldNative: number;
  /** valueNative − costNative; null when they are in different currencies. */
  gainNative: number | null;
  /** Today's value at the current quotes. */
  value: number | null;
  /** What the held quantity cost, every buy converted at its own day. */
  cost: number | null;
  gain: number | null;
  /** Gain relative to the purchase cost, 0–1 (0 when there is no cost; null when unknown). */
  gainPct: number | null;
  /** Yearly yield by the asset's yield type, at the current quotes. */
  yield: number | null;
}

/**
 * The held quantity's cost in the main currency. With the buy transactions it follows the
 * backend's average (sum of buys / bought quantity) with every buy converted at its own
 * day; an asset entered without transactions converts its purchase average on the day it
 * was created. Null when any of those days has no rate.
 */
function heldCost(
  asset: OtherAsset,
  quantity: number,
  costNative: number,
  currency: string,
  convertAt: ConvertAtFn,
  buys: readonly OtherAssetBuy[] | undefined
): number | null {
  if (costNative === 0) return 0;
  if (!buys || buys.length === 0) return convertAt(costNative, currency, asset.createdAt);
  let bought = 0;
  let spent = 0;
  for (const buy of buys) {
    const value = convertAt(buy.quantity * buy.pricePerUnit, buy.currency || currency, buy.date);
    if (value === null) return null;
    bought += buy.quantity;
    spent += value;
  }
  return bought > 0 ? quantity * (spent / bought) : 0;
}

export function otherAssetRow(
  asset: OtherAsset,
  convert: ConvertFn,
  convertAt: ConvertAtFn,
  buys?: readonly OtherAssetBuy[]
): OtherAssetRow {
  const quantity = parseFloat(asset.quantity) || 0;
  const price = parseFloat(asset.marketPrice) || 0;
  const avgPurchase = parseFloat(asset.averagePurchasePrice) || 0;
  const currency = recordCurrency(asset.currency);
  const priceCurrency = recordCurrency(asset.marketPriceCurrency);
  const yieldCurrency = asset.yieldType === 'percent_market' ? priceCurrency : currency;
  const valueNative = quantity * price;
  const costNative = quantity * avgPurchase;
  const yieldNative = calculateAnnualYield({
    yieldType: asset.yieldType as YieldType,
    yieldValue: parseFloat(asset.yieldValue || '0') || 0,
    quantity,
    averagePurchasePrice: avgPurchase,
    marketPrice: price,
  });
  const toMain = (amount: number, from: string) => (amount === 0 ? 0 : convert(amount, from));
  const value = toMain(valueNative, priceCurrency);
  const cost = heldCost(asset, quantity, costNative, currency, convertAt, buys);
  const gain = value !== null && cost !== null ? value - cost : null;
  return {
    asset,
    currency,
    priceCurrency,
    yieldCurrency,
    quantity,
    price,
    avgPurchase,
    valueNative,
    costNative,
    yieldNative,
    gainNative: priceCurrency === currency ? valueNative - costNative : null,
    value,
    cost,
    gain,
    gainPct: gain === null ? null : cost !== null && cost > 0 ? gain / cost : 0,
    yield: toMain(yieldNative, yieldCurrency),
  };
}

export interface OtherAssetsSummary {
  /** Sums of the known values, costs, gains and yields (main currency). */
  value: number;
  cost: number;
  gain: number;
  /** The cost of the assets whose gain is known (what `gain` is relative to). */
  gainCost: number;
  yield: number;
  /** Assets with a positive yearly yield. */
  yielding: number;
  /** Currencies left out of the values and yields (current quotes). */
  missing: string[];
  /** Currencies whose purchase day had no rate (left out of the cost and gain). */
  missingCost: string[];
}

/** Totals of the rows: every known amount is summed, the gain only where both sides are known. */
export function otherAssetsSummary(rows: readonly OtherAssetRow[]): OtherAssetsSummary {
  const missing = new MissingTracker();
  const missingCost = new MissingTracker();
  const totals = { value: 0, cost: 0, gain: 0, gainCost: 0, yield: 0, yielding: 0 };
  for (const r of rows) {
    totals.value += missing.track(r.priceCurrency, r.value) ?? 0;
    totals.yield += missing.track(r.yieldCurrency, r.yield) ?? 0;
    totals.cost += missingCost.track(r.currency, r.cost) ?? 0;
    if (r.gain !== null) {
      totals.gain += r.gain;
      totals.gainCost += r.cost ?? 0;
    }
    if (r.yieldNative > 0) totals.yielding += 1;
  }
  return { ...totals, missing: missing.missing, missingCost: missingCost.missing };
}
