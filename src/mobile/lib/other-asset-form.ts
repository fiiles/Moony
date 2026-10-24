/**
 * The logic of the add-asset sheet (spec §8.8, §10.5): what the form's text means, which errors it
 * has and what it sends. The desktop's `AddOtherAssetModal` holds the same rules inline in its
 * mutation; here they are a pure function so the sheet only renders them.
 */
import type { InsertOtherAsset, InsertOtherAssetTransaction } from '@shared/schema';
import { todayIsoUtc, utcDayStart } from '@/utils/period';
import { parseAmountInput } from './amount-input';

/** The yield types the backend knows, in the order the sheet lists them. */
export const YIELD_TYPES = ['none', 'fixed', 'percent_purchase', 'percent_market'] as const;
export type YieldType = (typeof YIELD_TYPES)[number];

/** What the sheet's fields hold: the amounts as typed, the date as `YYYY-MM-DD`. */
export interface OtherAssetFormInput {
  name: string;
  currency: string;
  /** Today's price per unit. */
  priceText: string;
  /** The first purchase: the quantity, the price per unit and the day. */
  quantityText: string;
  purchasePriceText: string;
  purchaseDate: string;
  yieldType: YieldType;
  yieldText: string;
}

export type AmountError = 'required' | 'invalid' | 'positive' | 'negative';

export interface OtherAssetFormState {
  errors: {
    name?: 'required';
    price?: AmountError;
    quantity?: AmountError;
    purchasePrice?: AmountError;
    date?: 'required' | 'future';
    yield?: 'invalid' | 'negative';
  };
  valid: boolean;
  /** An error sits in a folded field: the sheet unfolds so that it can be seen. */
  foldError: boolean;
  /** What to send once `valid`; `null` until then. */
  payload: { data: InsertOtherAsset; initialTransaction?: InsertOtherAssetTransaction } | null;
}

/** A typed amount as the backend's decimal text and as a number; `null` for no number. */
function amount(text: string): { text: string; value: number } | null {
  const parsed = parseAmountInput(text);
  const value = parsed === null ? NaN : Number(parsed);
  return parsed !== null && Number.isFinite(value) ? { text: parsed, value } : null;
}

/**
 * Reads the form. The name and today's price are required (a price of 0 is allowed, as the backend
 * does). The first purchase is begun by typing a quantity or a price: it then needs both, above
 * zero, and a day that is not after `today` (UTC, as the backend counts it) — half of one is named,
 * not dropped silently — and goes with the asset as its first buy; left empty the asset starts with
 * quantity 0 and no purchase average, as on the desktop. The yield value is optional; typed with a
 * type of "none" it is not sent.
 */
export function otherAssetFormState(
  form: OtherAssetFormInput,
  today: string = todayIsoUtc()
): OtherAssetFormState {
  const errors: OtherAssetFormState['errors'] = {};

  const name = form.name.trim();
  if (name === '') errors.name = 'required';

  const price = amount(form.priceText);
  if (form.priceText.trim() === '') errors.price = 'required';
  else if (price === null) errors.price = 'invalid';
  else if (price.value < 0) errors.price = 'negative';

  const quantity = amount(form.quantityText);
  const purchasePrice = amount(form.purchasePriceText);
  const begun = form.quantityText.trim() !== '' || form.purchasePriceText.trim() !== '';
  if (begun) {
    if (form.quantityText.trim() === '') errors.quantity = 'required';
    else if (quantity === null) errors.quantity = 'invalid';
    else if (quantity.value <= 0) errors.quantity = 'positive';

    if (form.purchasePriceText.trim() === '') errors.purchasePrice = 'required';
    else if (purchasePrice === null) errors.purchasePrice = 'invalid';
    else if (purchasePrice.value <= 0) errors.purchasePrice = 'positive';

    if (!/^\d{4}-\d{2}-\d{2}$/.test(form.purchaseDate)) errors.date = 'required';
    else if (form.purchaseDate > today) errors.date = 'future';
  }

  const yielded = amount(form.yieldText);
  if (form.yieldType !== 'none' && form.yieldText.trim() !== '') {
    if (yielded === null) errors.yield = 'invalid';
    else if (yielded.value < 0) errors.yield = 'negative';
  }

  const valid = Object.keys(errors).length === 0;
  const foldError = Boolean(errors.quantity || errors.purchasePrice || errors.date || errors.yield);
  if (!valid || price === null) return { errors, valid, foldError, payload: null };

  const purchase = begun && quantity !== null && purchasePrice !== null;
  return {
    errors,
    valid,
    foldError,
    payload: {
      data: {
        name,
        currency: form.currency,
        marketPrice: price.text,
        quantity: purchase ? quantity.text : '0',
        averagePurchasePrice: purchase ? purchasePrice.text : '0',
        yieldType: form.yieldType,
        yieldValue: form.yieldType === 'none' || yielded === null ? undefined : yielded.text,
      },
      initialTransaction: purchase
        ? {
            type: 'buy',
            quantity: quantity.text,
            pricePerUnit: purchasePrice.text,
            currency: form.currency,
            transactionDate: utcDayStart(form.purchaseDate),
          }
        : undefined,
    },
  };
}
