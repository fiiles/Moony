import { completeAccountIdentifier } from './iban-utils';

interface TransactionText {
  counterpartyName: string | null;
  counterpartyIban: string | null;
  counterpartyBankCode: string | null;
  description: string | null;
  remittanceInfo: string | null;
}

const clean = (value: string | null) => value?.trim() || null;
const key = (value: string) => value.toLocaleLowerCase().replace(/\s+/g, ' ');

/**
 * Who the money went to or came from: the name over the full account number,
 * or the account number alone when the name is unknown.
 */
export function counterpartyLabel(tx: TransactionText): {
  title: string | null;
  subtitle: string | null;
} {
  const name = clean(tx.counterpartyName);
  const account = completeAccountIdentifier(tx.counterpartyIban, null, tx.counterpartyBankCode);
  return name ? { title: name, subtitle: account } : { title: account, subtitle: null };
}

/** What the payment was for: the description and the remittance info, each once. */
export function transactionDetails(tx: TransactionText): string[] {
  const seen = new Set<string>();
  const details: string[] = [];
  for (const value of [clean(tx.description), clean(tx.remittanceInfo)]) {
    if (!value || seen.has(key(value))) continue;
    seen.add(key(value));
    details.push(value);
  }
  return details;
}
