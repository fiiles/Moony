import { useTranslation } from 'react-i18next';
import type { InferredColumns } from '@shared/schema';
import {
  usesDebitCredit,
  type MappingState,
} from '@/components/bank-accounts/csv-import/import-config';
import { dateOrderOf, wasInferred, type RecognitionField } from '@/mobile/lib/recognition';

/**
 * The words of the guided recognition's fields: their names (the desktop's own strings where it
 * has them) and what a field says about itself — the columns it reads, or why it is not used and
 * what that means for the import.
 */
export function useFieldText() {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');

  const name = (field: RecognitionField): string => {
    switch (field) {
      case 'date':
        return tb('transaction.date');
      case 'amount':
        return tb('transaction.amount');
      case 'description':
        return tb('transaction.description');
      case 'counterparty':
        return tb('transaction.counterparty');
      case 'currency':
        return tb('fields.currency');
      case 'balance':
        return tb('fields.balance');
      case 'transactionId':
        return t('import.recognize.field.transactionId');
    }
  };

  /** A debit and credit pair with its sides named: "Výdaje: Odchozí · Příjmy: Příchozí". */
  const pairSides = (sides: { debit: string; credit: string }): string =>
    t('import.recognize.pairSides', sides);

  /**
   * "Datum zaúčtování · den.měsíc.rok" for a field that is read ("Výdaje: Odchozí · Příjmy:
   * Příchozí" for an amount read from a pair: which side is the money out), "nenalezeno · duplicity
   * poznám podle data a částky" for one that is not (the inference found nothing) and "nepoužito ·
   * …" for one the person left out.
   */
  const value = (
    field: RecognitionField,
    columns: readonly string[],
    mapping: MappingState,
    inferred: InferredColumns,
    /** The sentence on what a field that is not read costs; the list of fields leaves it to the recognition. */
    consequence = true
  ): string => {
    if (columns.length > 0) {
      if (field === 'amount' && usesDebitCredit(mapping)) {
        return pairSides({ debit: mapping.debitColumn, credit: mapping.creditColumn });
      }
      const order = field === 'date' ? dateOrderOf(mapping, inferred) : null;
      return [...columns, order && tb(`statementImport.dateOrder.${order}`)]
        .filter(Boolean)
        .join(' · ');
    }
    const state = wasInferred(field, inferred)
      ? t('import.recognize.unused')
      : t('import.recognize.notFound');
    // A date or an amount is never left unused: the recognition asks for it first
    return field === 'date' || field === 'amount' || !consequence
      ? state
      : `${state} · ${t(`import.recognize.unusedWhy.${field}`)}`;
  };

  return { name, value, pairSides };
}
