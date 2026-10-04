import { z } from 'zod';

/**
 * Number field for react-hook-form + zod (v4).
 *
 * `<input type="number">` yields a string (or `undefined` while untouched), which zod coerces
 * with `Number(...)`. An untouched or non-numeric field becomes `NaN`, for which zod's default
 * message is the developer text "Invalid input: expected number, received NaN". This
 * helper reports `requiredKey` (an i18n key such as `validation.amountRequired`) instead;
 * `FormMessage` translates keys that start with `validation.`.
 *
 * Chain the usual constraints on the result:
 * `requiredNumber('validation.amountRequired').positive('validation.amountPositive')`.
 * The `<number>` generic keeps the form input type a number, like the rest of the repo.
 */
export function requiredNumber(requiredKey: string) {
  return z.coerce.number<number>({ error: requiredKey });
}
