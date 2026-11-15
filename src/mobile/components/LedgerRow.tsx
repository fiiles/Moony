import { Fragment, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowRightLeft, Info, Tag, Trash2 } from 'lucide-react';
import type {
  BankTransaction,
  EffectiveTransactionFlow,
  TransactionCategory,
} from '@shared/schema';
import { useCategoryName } from '@/hooks/use-categories';
import { rowFlowLabel } from '@/utils/money-flows';
import { DropdownMenuSeparator } from '@/components/ui/dropdown-menu';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { transactionTitle } from '@/mobile/lib/transaction-title';
import { CategoryChip } from './cashflow/CategoryChip';
import { signedAmountText } from '@/mobile/lib/signed-amount';
import { EntityRow } from './EntityRow';
import { RowBadge } from './RowBadge';

interface LedgerRowProps {
  tx: BankTransaction;
  /** The row's category; none shows the "Bez kategorie" badge. */
  category: TransactionCategory | null | undefined;
  /** The row's effective meaning, once loaded (a transfer chip, the refund badge). */
  flow: EffectiveTransactionFlow | undefined;
  /** Tap and swipe right: the transaction sheet. */
  onOpen: () => void;
  /** Swipe left: the category picker, directly. */
  onCategorize: () => void;
  onDelete: () => void;
}

/**
 * A movement of the account detail's ledger (spec §8.4, prototype frame 04), as every payment row
 * of the phone (README §13, "Transactions", audit 2026-11-15 C3/C4): the category's chip (a
 * transfer's arrows or a tag while it has none), the title of `transactionTitle`, the sub of the
 * category with the badges (the day heads say the day, the screen is the account), and the signed
 * amount in its own currency (a credit in `gain` with "+"). The bank's note is the sheet's. Tap
 * and swipe right open the details, swipe left the category picker (README §13, Gestures); a long
 * press is the row menu, whose `···` stays for screen readers: the tap's sheet has all of it.
 */
export function LedgerRow({ tx, category, flow, onOpen, onCategorize, onDelete }: LedgerRowProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const moneyText = useMoneyText();
  const categoryName = useCategoryName();

  const credit = tx.type === 'credit';
  const title = transactionTitle(tx) ?? t('bank_accounts:transaction.counterparty');
  const refund = !!flow && rowFlowLabel(flow, credit, !!category) === 'refund';

  const parts: ReactNode[] = [
    category ? (
      categoryName(category)
    ) : (
      <RowBadge line="sub">{t('account.ledger.noCategory')}</RowBadge>
    ),
    tx.status === 'pending' && (
      <RowBadge line="sub">{t('bank_accounts:readiness.pending')}</RowBadge>
    ),
    refund && <RowBadge line="sub">{t('bank_accounts:readiness.flowRefund')}</RowBadge>,
  ].filter(Boolean);
  const sub = parts.map((part, i) => (
    <Fragment key={i}>
      {i > 0 && ' · '}
      {part}
    </Fragment>
  ));

  const chip = (
    <CategoryChip
      category={category}
      fallback={flow?.effectiveFlowKind === 'transfer' ? ArrowRightLeft : Tag}
    />
  );

  return (
    <EntityRow
      chip={chip}
      title={title}
      sub={sub}
      value={signedAmountText(moneyText, tx)}
      valueTone={credit ? 'gain' : undefined}
      onClick={onOpen}
      onSwipeLeft={onCategorize}
      swipeLeftLabel={t('account.ledger.categorize')}
      swipeLeftIcon={Tag}
      onSwipeRight={onOpen}
      swipeRightLabel={t('account.ledger.details')}
      swipeRightIcon={Info}
      menu={{
        label: tc('mobile:labelValue', { label: tc('labels.actions'), value: title }),
        children: (
          <>
            <RowMenuItem onSelect={onOpen}>
              <Info />
              {t('account.rowMenu.details')}
            </RowMenuItem>
            <RowMenuItem onSelect={onCategorize}>
              <Tag />
              {t('account.rowMenu.category')}
            </RowMenuItem>
            <DropdownMenuSeparator />
            <RowMenuItem variant="danger" onSelect={onDelete}>
              <Trash2 />
              {t('account.rowMenu.delete')}
            </RowMenuItem>
          </>
        ),
      }}
    />
  );
}
