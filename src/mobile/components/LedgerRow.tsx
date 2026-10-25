import { Fragment, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowRightLeft, Banknote, Ellipsis, Info, Tag, Trash2 } from 'lucide-react';
import type {
  BankTransaction,
  EffectiveTransactionFlow,
  TransactionCategory,
} from '@shared/schema';
import { useCategoryName } from '@/hooks/use-categories';
import { cn } from '@/lib/utils';
import { rowFlowLabel } from '@/utils/money-flows';
import { counterpartyLabel, transactionDetails } from '@/utils/transaction-text';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { initials } from '@/mobile/lib/initials';
import { signedAmountText } from '@/mobile/lib/signed-amount';
import { EntityRow, RowChip } from './EntityRow';

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

/** The menu items: 44 pt tall, body type (as `AgendaRow`'s menu). */
const MENU_ITEM = 'min-h-11 text-m-body [&_svg]:size-[18px]';

/**
 * A badge in the sub-line ("Bez kategorie", "Čekající", "Vratka"; `.m-row__badge`; `loss` for the
 * import preview's "Chyba"). Shared with the import preview rows. Its text line is
 * the badge's full 18 pt (not `leading-none`), so a caron or an accent above a capital ("Č") stays
 * inside the sub-line, whose `truncate` box clips at the badge's edges.
 */
export function RowBadge({ children, tone }: { children: ReactNode; tone?: 'loss' }) {
  return (
    <span
      className={cn(
        'inline-flex min-h-[18px] items-center rounded-r1 px-1.5 py-px align-[1px] text-m-caption font-semibold leading-[1.3]',
        tone === 'loss' ? 'bg-loss-soft text-loss' : 'bg-well-2 text-ink-3'
      )}
    >
      {children}
    </span>
  );
}

/**
 * A movement of the account detail's ledger (spec §8.4, prototype frame 04): the counterparty's
 * initials (a transfer's arrows, a credit tinted `gain`), its name, the sub "Kategorie · detail"
 * with the badges, and the signed amount in its own currency (a credit in `gain` with "+"). Tap
 * and swipe right open the details, swipe left the category picker; a long press is the row menu.
 */
export function LedgerRow({ tx, category, flow, onOpen, onCategorize, onDelete }: LedgerRowProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const moneyText = useMoneyText();
  const categoryName = useCategoryName();
  const [menuOpen, setMenuOpen] = useState(false);

  const credit = tx.type === 'credit';
  const details = transactionDetails(tx);
  const title =
    counterpartyLabel(tx).title ?? details[0] ?? t('bank_accounts:transaction.counterparty');
  const detail = details.filter((text) => text !== title).join(' · ');
  const refund = !!flow && rowFlowLabel(flow, credit, !!category) === 'refund';

  const parts: ReactNode[] = [
    category ? categoryName(category) : <RowBadge>{t('account.ledger.noCategory')}</RowBadge>,
    tx.status === 'pending' && <RowBadge>{t('bank_accounts:readiness.pending')}</RowBadge>,
    refund && <RowBadge>{t('bank_accounts:readiness.flowRefund')}</RowBadge>,
    detail,
  ].filter(Boolean);
  const sub = parts.map((part, i) => (
    <Fragment key={i}>
      {i > 0 && ' · '}
      {part}
    </Fragment>
  ));

  const chip = (
    <RowChip tone={credit ? 'gain' : 'default'}>
      {flow?.effectiveFlowKind === 'transfer' ? (
        <ArrowRightLeft strokeWidth={1.75} />
      ) : (
        initials(title) || <Banknote strokeWidth={1.75} />
      )}
    </RowChip>
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
      onLongPress={() => setMenuOpen(true)}
      accessory={
        <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
          {/* The menu's anchor, and its way in for a screen reader or a keyboard: hidden until focused. */}
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label={`${tc('labels.actions')}: ${title}`}
              className="sr-only right-2 top-1/2 z-[3] focus-visible:not-sr-only focus-visible:absolute focus-visible:right-2 focus-visible:top-1/2 focus-visible:inline-flex focus-visible:size-11 focus-visible:-translate-y-1/2 focus-visible:items-center focus-visible:justify-center focus-visible:rounded-[12px] focus-visible:bg-paper focus-visible:text-ink focus-visible:shadow-focus"
            >
              <Ellipsis aria-hidden className="size-5" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-[200px]">
            <DropdownMenuItem className={MENU_ITEM} onSelect={onOpen}>
              <Info />
              {t('account.rowMenu.details')}
            </DropdownMenuItem>
            <DropdownMenuItem className={MENU_ITEM} onSelect={onCategorize}>
              <Tag />
              {t('account.rowMenu.category')}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem variant="danger" className={MENU_ITEM} onSelect={onDelete}>
              <Trash2 />
              {t('account.rowMenu.delete')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      }
    />
  );
}
