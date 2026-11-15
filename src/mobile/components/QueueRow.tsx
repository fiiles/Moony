import { useId, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Banknote, Check, Clock, Info, Tag } from 'lucide-react';
import type { CategorizeQueueRow, TransactionCategory } from '@shared/schema';
import { useCategoryName } from '@/hooks/use-categories';
import { cn } from '@/lib/utils';
import { transactionDetails } from '@/utils/transaction-text';
import { transactionTitle } from '@/mobile/lib/transaction-title';
import { DropdownMenuSeparator } from '@/components/ui/dropdown-menu';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useTransactionDay } from '@/mobile/hooks/use-transaction-day';
import { SNOOZE_DAYS, suggestionLabelKey, type RowState } from '@/mobile/lib/categorize-queue';
import { initials } from '@/mobile/lib/initials';
import { signedAmountText } from '@/mobile/lib/signed-amount';
import { EntityRow, RowChip } from './EntityRow';
import { SwitchRow } from './SwitchRow';

/** What a remembered pick took along, shown once under the row that made it (spec §8.5). */
export interface QueueExpansion {
  /** How many other payments got the category. */
  count: number;
  /** Turned off, the payments are restored and the switch stays off. */
  restored: boolean;
  onRestore: () => void;
}

interface QueueRowProps {
  row: CategorizeQueueRow;
  state: RowState;
  /** The category the row's suggestion names (an open row with one). */
  suggested: TransactionCategory | undefined;
  /** The category the row ended with, when this page knows it (a done row). */
  doneCategory: TransactionCategory | undefined;
  expansion?: QueueExpansion;
  /** The dark pill: accept the suggestion. */
  onAccept: () => void;
  /** "Jiná…", "Vybrat kategorii…", a tap on the row and the swipe to the left: the category picker. */
  onPick: () => void;
  /** "Nechat" (the pill of a row without a suggestion) and the menu: hold the payment back for 30 days. */
  onSnooze: () => void;
  /** Swipe right and the menu: the transaction sheet. */
  onDetails: () => void;
}

type PillTone = 'plain' | 'dark' | 'ghost';

const PILL_TONES: Record<PillTone, string> = {
  plain: 'bg-well-2 text-ink active:bg-well-3',
  dark: 'bg-dark-grad text-ink-inverse shadow-dark active:bg-dark-grad-hover',
  ghost: 'border border-line-strong bg-transparent text-ink-2 active:bg-well',
};

/** A pill of the pill row (`.m-pillrow .m-pill`): 36 pt, its hit area reaches 44 pt, the label may end with an ellipsis. */
function PillButton({
  tone,
  onClick,
  label,
  children,
}: {
  tone: PillTone;
  onClick: () => void;
  /** The accessible name when the visible text is not all of it. */
  label?: string;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      className={cn(
        "relative inline-flex min-h-9 max-w-full min-w-0 items-center gap-1.5 rounded-full px-3.5 py-1 text-m-sub font-semibold [-webkit-touch-callout:none] after:absolute after:inset-x-0 after:-inset-y-1 after:content-[''] focus-visible:outline-none focus-visible:shadow-focus [&_svg]:size-3.5 [&_svg]:flex-none",
        PILL_TONES[tone]
      )}
    >
      {children}
    </button>
  );
}

/**
 * A payment of the categorize queue (spec §8.5, prototype frame 05): the counterparty's initials,
 * its name, the sub "den · detail" and the signed amount in its own currency, as the ledger shows
 * it — then, in a second block of the same row, the pills. An open row with a suggestion has the
 * dark pill (a check, the category, the source of the idea in small type) and "Jiná…"; one
 * without has "Vybrat kategorii…" and "Nechat". A done row shows the category it got in the
 * green pill on `well`, and may carry the expansion; a row put off says so, also on `well`.
 * A tap on the row and swipe left open the picker (a payment's swipe left is always Kategorie,
 * README §13 Gestures), swipe right the details; a long press or the visible `···` is the menu with
 * all three, Nechat included, so nothing is reachable by gesture alone.
 */
export function QueueRow({
  row,
  state,
  suggested,
  doneCategory,
  expansion,
  onAccept,
  onPick,
  onSnooze,
  onDetails,
}: QueueRowProps) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const dayText = useTransactionDay();
  const moneyText = useMoneyText();
  const categoryName = useCategoryName();

  const { transaction: tx, suggestion } = row;
  const credit = tx.type === 'credit';
  const details = transactionDetails(tx);
  // The title of every payment row (C4); the queue keeps the rest of the text in its sub, as the
  // words of the payment are what a category is chosen by
  const title = transactionTitle(tx) ?? tb('transaction.counterparty');
  const detail = details.filter((text) => text !== title).join(' · ');

  const sub = [dayText(tx.bookingDate), detail].filter(Boolean).join(' · ');

  const open = state === 'open';

  let pills: ReactNode;
  if (state === 'left') {
    pills = (
      <span className="inline-flex min-h-9 items-center gap-1.5 text-m-sub font-semibold text-ink-3 [&_svg]:size-3.5">
        <Clock aria-hidden strokeWidth={1.75} />
        {t('queue.snoozed', { days: SNOOZE_DAYS })}
      </span>
    );
  } else if (state === 'done') {
    pills = (
      <span className="inline-flex min-h-9 max-w-full min-w-0 items-center gap-1.5 rounded-full bg-gain-soft px-3.5 py-1 text-m-sub font-semibold text-gain [&_svg]:size-3.5 [&_svg]:flex-none">
        <Check aria-hidden strokeWidth={2} />
        <span className="truncate">
          {doneCategory ? categoryName(doneCategory) : t('queue.categorized')}
        </span>
      </span>
    );
  } else if (suggestion && suggested) {
    const name = categoryName(suggested);
    const source = t(suggestionLabelKey(suggestion.source));
    pills = (
      <>
        <PillButton
          tone="dark"
          onClick={onAccept}
          label={`${tb('categorization.acceptSuggestion')}: ${name} (${source})`}
        >
          <Check aria-hidden strokeWidth={2} />
          <span className="min-w-0 truncate">{name}</span>
          <small aria-hidden className="flex-none text-m-note text-ink-inverse-2">
            · {source}
          </small>
        </PillButton>
        <PillButton tone="ghost" onClick={onPick}>
          {t('queue.other')}
        </PillButton>
      </>
    );
  } else {
    pills = (
      <>
        <PillButton tone="plain" onClick={onPick}>
          <Tag aria-hidden strokeWidth={1.75} />
          {t('queue.choose')}
        </PillButton>
        <PillButton tone="ghost" onClick={onSnooze}>
          {t('queue.keep')}
        </PillButton>
      </>
    );
  }

  const below = (
    <>
      <div className="flex flex-wrap items-center gap-2 px-3.5 pb-3 pt-0.5">{pills}</div>
      {expansion && state === 'done' && <Expansion expansion={expansion} name={title} />}
    </>
  );

  return (
    <EntityRow
      className={cn(state !== 'open' && 'bg-well')}
      chip={
        <RowChip tone={credit ? 'gain' : 'default'}>
          {initials(title) || <Banknote strokeWidth={1.75} />}
        </RowChip>
      }
      title={title}
      sub={sub}
      value={signedAmountText(moneyText, tx)}
      valueTone={credit ? 'gain' : undefined}
      onClick={onPick}
      onSwipeLeft={onPick}
      swipeLeftLabel={t('account.ledger.categorize')}
      swipeLeftIcon={Tag}
      onSwipeRight={onDetails}
      swipeRightLabel={t('account.ledger.details')}
      swipeRightIcon={Info}
      menu={{
        label: `${tc('labels.actions')}: ${title}`,
        // Podrobnosti (and Nechat on a row with a suggestion) are nowhere else on the row
        shown: true,
        children: (
          <>
            <RowMenuItem onSelect={onDetails}>
              <Info />
              {t('account.rowMenu.details')}
            </RowMenuItem>
            <RowMenuItem onSelect={onPick}>
              <Tag />
              {t('account.rowMenu.category')}
            </RowMenuItem>
            {open && (
              <>
                <DropdownMenuSeparator />
                <RowMenuItem onSelect={onSnooze}>
                  <Clock />
                  {t('queue.keepMenu', { days: SNOOZE_DAYS })}
                </RowMenuItem>
              </>
            )}
          </>
        ),
      }}
      below={below}
    />
  );
}

/**
 * "Zařazeny i další 3 platby od Lidl", once, under the row whose remembered pick took them along:
 * a switch, on. Turning it off restores them and the switch stays off (it has nothing left to
 * undo).
 */
function Expansion({ expansion, name }: { expansion: QueueExpansion; name: string }) {
  const { t } = useTranslation('mobile');
  const id = useId();
  const { count, restored, onRestore } = expansion;
  return (
    <div className="mx-3.5 mb-1 border-t border-line-soft pt-1">
      <SwitchRow
        id={id}
        label={t('queue.also', { count, name })}
        sub={restored ? t('queue.alsoRestored') : t('queue.alsoSub')}
        checked={!restored}
        disabled={restored}
        onCheckedChange={(checked) => {
          if (!checked) onRestore();
        }}
      />
    </div>
  );
}
