/**
 * IBAN utility functions for Czech account number formatting
 */

/**
 * Check if an IBAN is a Czech IBAN
 */
export function isCzechIBAN(iban: string | null | undefined): boolean {
  if (!iban) return false;
  const cleaned = iban.replace(/\s/g, '').toUpperCase();
  return cleaned.startsWith('CZ');
}

/**
 * Convert Czech IBAN to BBAN format (account/bank_code)
 *
 * Czech IBAN format: CZ + 2 check digits + 4-digit bank code + 16-digit account
 * Example: CZ6508000000192000145399 → 192000145399/0800
 *
 * The 16-digit account number consists of:
 * - 6-digit prefix (can be all zeros)
 * - 10-digit base account number
 *
 * Output format: [prefix-]account/bank (prefix shown only if non-zero)
 */
export function ibanToBBAN(iban: string | null | undefined): string | null {
  if (!iban) return null;

  const cleaned = iban.replace(/\s/g, '').toUpperCase();

  // Must be Czech IBAN with correct length (24 chars)
  if (!cleaned.startsWith('CZ') || cleaned.length !== 24) {
    return null;
  }

  // Extract parts: CZ(2) + check(2) + bank(4) + account(16)
  const bankCode = cleaned.substring(4, 8);
  const prefix = cleaned.substring(8, 14);
  const account = cleaned.substring(14, 24);

  // Remove leading zeros from account parts
  const prefixNum = prefix.replace(/^0+/, '');
  const accountNum = account.replace(/^0+/, '') || '0';

  // Format: prefix-account/bank or account/bank
  if (prefixNum) {
    return `${prefixNum}-${accountNum}/${bankCode}`;
  }
  return `${accountNum}/${bankCode}`;
}

/**
 * Format IBAN for display - either as BBAN (for Czech) or original IBAN
 */
export function formatAccountNumber(
  iban: string | null | undefined,
  preferBBAN: boolean = false
): string | null {
  if (!iban) return null;

  if (preferBBAN && isCzechIBAN(iban)) {
    return ibanToBBAN(iban);
  }

  // For non-Czech IBANs, format with spaces for readability
  const cleaned = iban.replace(/\s/g, '').toUpperCase();
  return cleaned.replace(/(.{4})/g, '$1 ').trim();
}
