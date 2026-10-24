import { useId, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, ChevronUp } from 'lucide-react';
import type {
  BankTransaction,
  EffectiveTransactionFlow,
  FlowKind,
  TransactionCategory,
} from '@shared/schema';
import { useCategoryName } from '@/hooks/use-categories';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { counterpartyLabel, transactionDetails } from '@/utils/transaction-text';
import { Button } from '@/components/ui/button';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { Segment } from '@/mobile/components/Segment';
import { Sheet } from '@/mobile/components/Sheet';
import { useDeleteDescription } from '@/mobile/hooks/use-delete-description';
import { useFitText } from '@/mobile/hooks/use-fit-text';
import { useLedgerMutations } from '@/mobile/hooks/use-ledger-mutations';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useRetained } from '@/mobile/hooks/use-retained';
import { signedAmountText } from '@/mobile/lib/signed-amount';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';

interface TransactionSheetProps {
  /** The row to show; `null` while the sheet is closed (it keeps the last one while it slides away). */
  tx: BankTransaction | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The row's category, when it has one. */
  category: TransactionCategory | undefined;
  /** The row's effective meaning, once loaded. */
  flow: EffectiveTransactionFlow | undefined;
  /** Opens the category picker over this sheet. */
  onPickCategory: () => void;
  /** Says what the sheet has changed on the row, so that the copy the caller holds follows. */
  onChange?: (patch: Partial<BankTransaction>) => void;
  /** The payment has been deleted (the sheet closes itself); for a list that holds its own copy of the row. */
  onDeleted?: () => void;
}

/** The segment's choices: the automatic meaning, or one of the three roles. */
type FlowChoice = 'auto' | FlowKind;
const FLOW_CHOICES: readonly FlowChoice[] = ['auto', 'income', 'expense', 'transfer'];

/**
 * The transaction sheet (spec §8.4, README §13): the amount, the counterparty, the date, the
 * category (a row that opens the picker over this sheet), the meaning of the movement (Automaticky
 * or one of the roles, with what the automatic meaning comes to), the note, and the technical
 * details folded under "Podrobnosti". Smazat asks first in a confirm sheet.
 */
export function TransactionSheet({
  tx,
  open,
  onOpenChange,
  category,
  flow,
  onPickCategory,
  onChange,
  onDeleted,
}: TransactionSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const categoryName = useCategoryName();
  const { setFlowKind, deleteTransaction } = useLedgerMutations();
  const describeDelete = useDeleteDescription();
  const shown = useRetained(tx);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const detailsId = useId();

  if (!shown) return null;
  // Every opening starts with the details folded
  const changeOpen = (next: boolean) => {
    if (!next) setExpanded(false);
    onOpenChange(next);
  };
  const close = () => changeOpen(false);
  const label = counterpartyLabel(shown);
  const details = transactionDetails(shown);
  // The row's own title (as the ledger shows it), the note is what is left of the description
  const title = label.title ?? details[0] ?? tb('transaction.counterparty');
  const note = details.filter((text) => text !== title);
  const auto = shown.flowKind === null;

  const flowOptions = FLOW_CHOICES.map((choice) => ({
    value: choice,
    label: tb(`readiness.flow.${choice}`),
  }));
  const meaning =
    auto && flow
      ? `${tb(`readiness.flow.${flow.effectiveFlowKind}`)} · ${tb(`readiness.source.${flow.flowSource}`)}`
      : undefined;
  const changeFlow = (choice: FlowChoice) => {
    const flowKind = choice === 'auto' ? null : choice;
    setFlowKind.mutate({ id: shown.id, flowKind }, { onSuccess: () => onChange?.({ flowKind }) });
  };

  const technical: [string, ReactNode][] = [
    [t('transaction.bankId'), shown.transactionId],
    [t('transaction.valueDate'), shown.valueDate === null ? null : fmt.day(shown.valueDate)],
    [t('transaction.file'), shown.importFileName],
    [t('transaction.line'), shown.importLine],
    [t('transaction.sourceLabel'), t(`transaction.source.${shown.dataSource}`)],
    [
      t('transaction.status'),
      shown.status === 'pending' ? tb('readiness.pending') : t('transaction.booked'),
    ],
    [t('transaction.currency'), shown.currency],
  ];

  return (
    <>
      <Sheet
        full
        open={open}
        onOpenChange={changeOpen}
        title={title}
        description={fmt.day(shown.bookingDate, { day: 'numeric', month: 'long', year: 'numeric' })}
        footer={
          <>
            <Button
              variant="ghost"
              className="h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] px-[18px] text-m-row text-loss"
              onClick={() => setConfirmOpen(true)}
            >
              {tb('transaction.delete')}
            </Button>
            <Button
              className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
              onClick={close}
            >
              {t('transaction.done')}
            </Button>
          </>
        }
      >
        <Amount tx={shown} />
        <div className="divide-y divide-line-soft">
          {label.title && (
            <KeyValueRow
              label={t('transaction.counterparty')}
              value={label.title}
              sub={label.subtitle}
            />
          )}
          <KeyValueRow label={t('transaction.date')} value={fmt.day(shown.bookingDate)} />
          <KeyValueRow
            label={t('transaction.category')}
            value={category ? categoryName(category) : t('account.ledger.noCategory')}
            onClick={onPickCategory}
          />
          <KeyValueRow
            stack
            label={t('transaction.flow')}
            value={
              <Segment<FlowChoice>
                value={shown.flowKind ?? 'auto'}
                options={flowOptions}
                onChange={changeFlow}
                label={t('transaction.flow')}
              />
            }
            sub={meaning}
          />
          {note.length > 0 && (
            <KeyValueRow stack label={t('transaction.note')} value={note.join('\n')} />
          )}
        </div>
        <button
          type="button"
          aria-expanded={expanded}
          aria-controls={detailsId}
          onClick={() => setExpanded(!expanded)}
          className="flex min-h-11 w-full items-center justify-between px-1 py-2 text-left text-m-body font-semibold text-ink-2 [-webkit-touch-callout:none] active:bg-well focus-visible:outline-none focus-visible:shadow-focus"
        >
          {t('transaction.details')}
          {expanded ? (
            <ChevronUp aria-hidden className="size-4 text-ink-4" strokeWidth={1.75} />
          ) : (
            <ChevronDown aria-hidden className="size-4 text-ink-4" strokeWidth={1.75} />
          )}
        </button>
        {expanded && (
          <div id={detailsId} className="divide-y divide-line-soft">
            {technical
              .filter(([, value]) => value !== null)
              .map(([name, value]) => (
                <KeyValueRow key={name} label={name} value={value} />
              ))}
          </div>
        )}
      </Sheet>
      <ConfirmSheet
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={tb('confirmDeleteTransaction.title')}
        description={describeDelete(shown)}
        confirmLabel={tb('transaction.delete')}
        onConfirm={() =>
          deleteTransaction.mutate(shown.id, {
            onSuccess: () => {
              setConfirmOpen(false);
              close();
              onDeleted?.();
            },
          })
        }
        isPending={deleteTransaction.isPending}
      />
    </>
  );
}

/**
 * The amount, signed and in its own currency (a credit in `gain`), as one line in the display
 * type (`.m-sheet__amount`): a long figure shrinks to fit rather than end in an ellipsis.
 */
function Amount({ tx }: { tx: BankTransaction }) {
  const moneyText = useMoneyText();
  const ref = useRef<HTMLDivElement>(null);
  const credit = tx.type === 'credit';
  const text = signedAmountText(moneyText, tx);
  const fit = useFitText(ref, text);

  return (
    <div ref={ref} className="px-1 pb-2 pt-1">
      <div
        data-fit-text
        className={cn(
          'overflow-hidden text-ellipsis whitespace-nowrap text-m-display num',
          credit ? 'text-gain' : 'text-ink'
        )}
        style={
          fit === null
            ? undefined
            : { fontSize: `clamp(var(--m-fs-title), ${fit}px, var(--m-fs-display))` }
        }
      >
        <span>{text}</span>
      </div>
    </div>
  );
}
