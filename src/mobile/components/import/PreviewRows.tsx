import { Fragment, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Banknote } from 'lucide-react';
import type { CsvPreviewRow, TransactionCategory } from '@shared/schema';
import { useCategoryName } from '@/hooks/use-categories';
import { useFormat } from '@/lib/use-format';
import { translateRowMessage } from '@/utils/csv-import-messages';
import { counterpartyLabel, transactionDetails } from '@/utils/transaction-text';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { RowBadge } from '@/mobile/components/RowBadge';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { initials } from '@/mobile/lib/initials';
import { previewAmount } from '@/mobile/lib/import-flow';
import { signedAmountText } from '@/mobile/lib/signed-amount';

interface PreviewRowsProps {
  /** The rows to show (the first of the file), in file order. */
  rows: readonly CsvPreviewRow[];
  /** Every row of the file: the rows that are not shown are counted under the list. */
  total: number;
  /** The target account's currency, for a row whose file has no currency column. */
  accountCurrency: string;
  categories?: readonly TransactionCategory[];
  /** The category the person chose in this preview, by the row's `line`. */
  overrides?: ReadonlyMap<number, string>;
  /** A row that will be written was tapped: its category is to be chosen. */
  onPick?: (row: CsvPreviewRow) => void;
  /** The list on screen belongs to older choices and is being replaced. */
  updating: boolean;
  /**
   * The rows are the guided recognition's samples ("Takhle tomu rozumím"): the sub-line says
   * "date · what it was for" instead of the category, a row is never a duplicate or tappable —
   * the file is only being read — and one that cannot be read says so.
   */
  sample?: boolean;
}

/** A row the import will write: its category can be chosen. */
const willWrite = (row: Pick<CsvPreviewRow, 'status'>) =>
  row.status === 'ok' || row.status === 'promoted';

/**
 * The preview's rows (README §13, "Import preview rows"): the entity row of a movement — the
 * counterparty's initials (a credit's chip in `gain`), its name, "date · category" in the sub-line
 * ("Bez kategorie" as the ledger's badge) and the signed amount in its own currency, masked while
 * amounts are hidden with the sign kept. A row the import skips (a duplicate, a zero amount) is at
 * half strength with its reason as a badge, an error row says what is wrong with it; a row that
 * will be written opens the category choice on a tap. "a N dalších" counts what is not shown.
 * `sample` draws the three rows of the recognition the same way with their description in place
 * of the category (README §13, "Import recognition").
 */
export function PreviewRows({
  rows,
  total,
  accountCurrency,
  categories = [],
  overrides = new Map<number, string>(),
  onPick,
  updating,
  sample = false,
}: PreviewRowsProps) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const categoryName = useCategoryName();
  const categoryById = new Map(categories.map((category) => [category.id, category]));

  const rowFor = (row: CsvPreviewRow) => {
    const text = {
      counterpartyName: row.counterparty,
      counterpartyIban: row.counterpartyIban,
      counterpartyBankCode: row.counterpartyBankCode,
      description: row.description,
      remittanceInfo: null,
    };
    const details = transactionDetails(text);
    const title = counterpartyLabel(text).title ?? details[0] ?? tb('transaction.counterparty');
    const amount = previewAmount(row, accountCurrency);
    const credit = amount?.type === 'credit';
    const picked = overrides.get(row.line);
    const category = categoryById.get(picked ?? row.categoryId ?? '');
    // A sample is how the file reads, whatever the account already holds: only a row that cannot be read stands apart
    const writes = sample ? row.status !== 'error' : willWrite(row);
    const message = row.message ? translateRowMessage(row.message, tb, tc) : null;

    const parts: ReactNode[] = [
      row.bookingDate !== null ? fmt.day(row.bookingDate) : '—',
      writes &&
        (category ? (
          picked !== undefined ? (
            `${categoryName(category)} · ${tb('csvImport.preview.manual')}`
          ) : (
            categoryName(category)
          )
        ) : (
          <RowBadge line="sub">{t('account.ledger.noCategory')}</RowBadge>
        )),
      row.status === 'promoted' && (
        <RowBadge line="sub">{tb('csvImport.status.promoted')}</RowBadge>
      ),
      row.status === 'duplicate' && (
        <RowBadge line="sub">{tb('csvImport.status.duplicate')}</RowBadge>
      ),
      row.status === 'skipped' && <RowBadge line="sub">{tb('csvImport.status.skipped')}</RowBadge>,
      row.status === 'error' && (
        <RowBadge line="sub" tone="loss">
          {tb('csvImport.status.error')}
        </RowBadge>
      ),
      row.status !== 'ok' && row.status !== 'promoted' && message,
    ].filter(Boolean);
    // The recognition's rows: what the payment was for stands where the category does
    const sampleParts: ReactNode[] = [
      row.bookingDate !== null ? fmt.day(row.bookingDate) : '—',
      details.find((detail) => detail !== title),
      row.status === 'error' && (
        <RowBadge line="sub" tone="loss">
          {tb('csvImport.status.error')}
        </RowBadge>
      ),
      row.status === 'error' && message,
    ].filter(Boolean);

    return (
      <EntityRow
        key={row.line}
        chip={
          <RowChip tone={credit && writes ? 'gain' : 'default'}>
            {initials(title) || <Banknote strokeWidth={1.75} />}
          </RowChip>
        }
        title={title}
        sub={(sample ? sampleParts : parts).map((part, i) => (
          <Fragment key={i}>
            {i > 0 && ' · '}
            {part}
          </Fragment>
        ))}
        value={amount ? signedAmountText(moneyText, amount) : '—'}
        valueTone={credit && writes ? 'gain' : undefined}
        muted={!sample && (row.status === 'duplicate' || row.status === 'skipped')}
        onClick={!sample && writes && onPick ? () => onPick(row) : undefined}
      />
    );
  };

  return (
    <div aria-busy={updating} className={updating ? 'opacity-60 transition-opacity' : undefined}>
      <EntityRows className="mb-2">{rows.map(rowFor)}</EntityRows>
      {total > rows.length && (
        <p className="mx-1 mb-3 mt-0 text-m-sub text-ink-3">
          {t('import.preview.more', { count: total - rows.length })}
        </p>
      )}
    </div>
  );
}
