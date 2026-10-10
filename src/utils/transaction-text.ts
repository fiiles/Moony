interface TransactionText {
  counterpartyName: string | null;
  description: string | null;
  remittanceInfo: string | null;
}

const clean = (value: string | null) => value?.trim() || null;
const key = (value: string) => value.toLocaleLowerCase().replace(/\s+/g, ' ');

/** Row headline: the counterparty, else the bank's description of the movement. */
export function transactionTitle(tx: TransactionText): string | null {
  return clean(tx.counterpartyName) ?? clean(tx.description);
}

/** Free-text details of a row, without repeating the headline or each other. */
export function transactionDetails(tx: TransactionText): string[] {
  const title = transactionTitle(tx);
  const seen = new Set(title ? [key(title)] : []);
  const details: string[] = [];
  for (const value of [clean(tx.description), clean(tx.remittanceInfo)]) {
    if (!value || seen.has(key(value))) continue;
    seen.add(key(value));
    details.push(value);
  }
  return details;
}
