import { describe, expect, it } from 'vitest';
import { otherAssetFormState, type OtherAssetFormInput } from './other-asset-form';

const input = (patch: Partial<OtherAssetFormInput> = {}): OtherAssetFormInput => ({
  name: 'Zlaté slitky',
  currency: 'CZK',
  priceText: '1 850,50',
  quantityText: '',
  purchasePriceText: '',
  purchaseDate: '2026-03-01',
  yieldType: 'none',
  yieldText: '',
  ...patch,
});

describe('otherAssetFormState', () => {
  it('needs a name and a price, nothing else', () => {
    const state = otherAssetFormState(input());
    expect(state.valid).toBe(true);
    expect(state.errors).toEqual({});
    expect(state.payload).toEqual({
      data: {
        name: 'Zlaté slitky',
        currency: 'CZK',
        marketPrice: '1850.50',
        quantity: '0',
        averagePurchasePrice: '0',
        yieldType: 'none',
        yieldValue: undefined,
      },
      initialTransaction: undefined,
    });
  });

  it('names what is missing or wrong in the required fields', () => {
    const state = otherAssetFormState(input({ name: '   ', priceText: '' }));
    expect(state.valid).toBe(false);
    expect(state.errors).toEqual({ name: 'required', price: 'required' });
    expect(state.payload).toBeNull();
    expect(otherAssetFormState(input({ priceText: '12x' })).errors).toEqual({ price: 'invalid' });
    expect(otherAssetFormState(input({ priceText: '-5' })).errors).toEqual({ price: 'negative' });
    // A price of 0 is a price the backend takes
    expect(otherAssetFormState(input({ priceText: '0' })).valid).toBe(true);
  });

  it('sends the first purchase when quantity and price are filled, dated on its UTC day', () => {
    const state = otherAssetFormState(
      input({ quantityText: '2,5', purchasePriceText: '1 700', purchaseDate: '2026-03-01' })
    );
    expect(state.valid).toBe(true);
    expect(state.payload?.data).toMatchObject({ quantity: '2.5', averagePurchasePrice: '1700' });
    expect(state.payload?.initialTransaction).toEqual({
      type: 'buy',
      quantity: '2.5',
      pricePerUnit: '1700',
      currency: 'CZK',
      transactionDate: Date.UTC(2026, 2, 1) / 1000,
    });
  });

  it('asks for the other half of a first purchase instead of dropping it silently', () => {
    const noPrice = otherAssetFormState(input({ quantityText: '2' }));
    expect(noPrice.errors).toEqual({ purchasePrice: 'required' });
    expect(noPrice.foldError).toBe(true);
    expect(noPrice.payload).toBeNull();
    expect(otherAssetFormState(input({ purchasePriceText: '9' })).errors).toEqual({
      quantity: 'required',
    });
  });

  it('wants a positive quantity and price and a date once a purchase is begun', () => {
    expect(
      otherAssetFormState(input({ quantityText: '0', purchasePriceText: '-3' })).errors
    ).toEqual({ quantity: 'positive', purchasePrice: 'positive' });
    expect(
      otherAssetFormState(input({ quantityText: 'a', purchasePriceText: '1', purchaseDate: '' }))
        .errors
    ).toEqual({ quantity: 'invalid', date: 'required' });
  });

  it('ignores the date while there is no purchase', () => {
    expect(otherAssetFormState(input({ purchaseDate: '' })).valid).toBe(true);
    // Not even a day to come: with no purchase there is nothing to date
    expect(otherAssetFormState(input({ purchaseDate: '2099-01-01' })).valid).toBe(true);
  });

  it('refuses a first-purchase day after today (UTC) and takes today itself', () => {
    const purchase = { quantityText: '2', purchasePriceText: '5' };
    const future = otherAssetFormState(
      input({ ...purchase, purchaseDate: '2026-10-25' }),
      '2026-10-24'
    );
    expect(future.valid).toBe(false);
    expect(future.errors).toEqual({ date: 'future' });
    expect(future.foldError).toBe(true);
    expect(future.payload).toBeNull();
    expect(
      otherAssetFormState(input({ ...purchase, purchaseDate: '2026-10-24' }), '2026-10-24').valid
    ).toBe(true);
    expect(
      otherAssetFormState(input({ ...purchase, purchaseDate: '2026-10-23' }), '2026-10-24').valid
    ).toBe(true);
  });

  it('sends the yield by its type and leaves an empty value out', () => {
    const fixed = otherAssetFormState(input({ yieldType: 'fixed', yieldText: '1 200,5' }));
    expect(fixed.payload?.data).toMatchObject({ yieldType: 'fixed', yieldValue: '1200.5' });
    const empty = otherAssetFormState(input({ yieldType: 'percent_market', yieldText: '' }));
    expect(empty.valid).toBe(true);
    expect(empty.payload?.data).toMatchObject({
      yieldType: 'percent_market',
      yieldValue: undefined,
    });
    // A value typed and then the type set back to none is not sent
    const none = otherAssetFormState(input({ yieldType: 'none', yieldText: '5' }));
    expect(none.payload?.data).toMatchObject({ yieldType: 'none', yieldValue: undefined });
  });

  it('rejects a yield value that is not a number or is negative, in the fold', () => {
    const bad = otherAssetFormState(input({ yieldType: 'fixed', yieldText: 'abc' }));
    expect(bad.errors).toEqual({ yield: 'invalid' });
    expect(bad.foldError).toBe(true);
    expect(
      otherAssetFormState(input({ yieldType: 'percent_purchase', yieldText: '-2' })).errors
    ).toEqual({ yield: 'negative' });
  });

  it('flags a fold error only for the folded fields', () => {
    const state = otherAssetFormState(input({ name: '' }));
    expect(state.valid).toBe(false);
    expect(state.foldError).toBe(false);
  });
});
