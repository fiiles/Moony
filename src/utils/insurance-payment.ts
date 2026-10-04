import type { PaymentFrequency } from '@shared/calculations';

/**
 * Which payment inputs the insurance form shows for a frequency.
 *
 * A one-time policy has a single lump sum (required) and no regular payment; a recurring one
 * has the regular payment (required) and no one-time amount. The exception is a stored
 * recurring policy that already carries a one-time amount (`hasStoredOneTime`): that field
 * stays visible and optional so saving the form cannot wipe the value unseen.
 */
export function insurancePaymentFields(
  frequency: PaymentFrequency,
  hasStoredOneTime: boolean
): { showOneTime: boolean; oneTimeRequired: boolean; showRegular: boolean } {
  const oneTime = frequency === 'one_time';
  return {
    showOneTime: oneTime || hasStoredOneTime,
    oneTimeRequired: oneTime,
    showRegular: !oneTime,
  };
}

/**
 * The payment amounts to send to the backend: the inputs that were hidden for the chosen
 * frequency are reset (`regularPayment` to "0", the one-time amount to nothing) so a policy
 * never keeps a number the user can no longer see.
 */
export function normalizeInsurancePayments(input: {
  frequency: PaymentFrequency;
  oneTimePayment: string | undefined;
  regularPayment: string | undefined;
  /** Same flag as `hasStoredOneTime` of {@link insurancePaymentFields}. */
  keepOneTime: boolean;
}): { oneTimePayment: string | undefined; regularPayment: string } {
  const oneTime = input.frequency === 'one_time';
  const oneTimePayment =
    (oneTime || input.keepOneTime) && input.oneTimePayment?.trim()
      ? input.oneTimePayment
      : undefined;
  return {
    oneTimePayment,
    regularPayment: oneTime ? '0' : (input.regularPayment ?? '0'),
  };
}

export interface InsurancePaymentIssue {
  path: 'oneTimePayment' | 'regularPayment';
  /** `validation.*` key of `common.json`, translated by `FormMessage`. */
  key: string;
}

function amountIssue(
  value: string | undefined,
  path: InsurancePaymentIssue['path'],
  { required, positive }: { required: boolean; positive: boolean }
): InsurancePaymentIssue | null {
  const text = value?.trim() ?? '';
  if (text === '') return required ? { path, key: 'validation.amountRequired' } : null;
  const amount = Number(text);
  if (!Number.isFinite(amount)) return { path, key: 'validation.amountRequired' };
  if (amount < 0) return { path, key: 'validation.paymentNonNegative' };
  if (positive && amount === 0) return { path, key: 'validation.amountRequired' };
  return null;
}

/**
 * Validation of the visible payment inputs only: the one-time amount (positive) for a
 * one-time policy, the regular payment (zero or more) for a recurring one. The optional
 * one-time amount of a recurring policy only has to be non-negative.
 */
export function insurancePaymentIssues(
  frequency: PaymentFrequency,
  oneTimePayment: string | undefined,
  regularPayment: string | undefined
): InsurancePaymentIssue[] {
  const issues: Array<InsurancePaymentIssue | null> =
    frequency === 'one_time'
      ? [amountIssue(oneTimePayment, 'oneTimePayment', { required: true, positive: true })]
      : [
          amountIssue(regularPayment, 'regularPayment', { required: true, positive: false }),
          amountIssue(oneTimePayment, 'oneTimePayment', { required: false, positive: false }),
        ];
  return issues.filter((issue): issue is InsurancePaymentIssue => issue !== null);
}
