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

/**
 * Optional fee field: a string so the stored decimal text round-trips untouched.
 * Empty (or blank) is allowed; anything else must be a number >= 0. Messages are
 * i18n keys (`FormMessage` translates keys that start with `validation.`).
 */
export function optionalFee() {
  return z
    .string()
    .optional()
    .superRefine((value, ctx) => {
      const text = (value ?? '').trim();
      if (text === '') return;
      const amount = Number(text);
      if (!Number.isFinite(amount)) {
        ctx.addIssue({ code: 'custom', message: 'validation.feeInvalid' });
      } else if (amount < 0) {
        ctx.addIssue({ code: 'custom', message: 'validation.feeNegative' });
      }
    });
}

/** Wire value for a new transaction: a blank fee is omitted (no fee). */
export function feeForCreate(value: string | undefined): string | undefined {
  const text = (value ?? '').trim();
  return text === '' ? undefined : text;
}

/** Wire value for an update: a blank fee is `""`, which clears the stored one. */
export function feeForUpdate(value: string | undefined): string {
  return (value ?? '').trim();
}
