import { useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, ChevronUp, Copy } from 'lucide-react';
import { toast } from 'sonner';
import type {
  BankTransaction,
  EffectiveTransactionFlow,
  FlowKind,
  TransactionCategory,
} from '@shared/schema';
import { useCategoryName } from '@/hooks/use-categories';
import { useSpendingPlanIndex } from '@/hooks/use-spending-plans';
import { PlanChip } from '@/components/spending-plans/PlanChip';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { counterpartyLabel, transactionDetails } from '@/utils/transaction-text';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { Segment } from '@/mobile/components/Segment';
import { Sheet } from '@/mobile/components/Sheet';
import { useDeleteDescription } from '@/mobile/hooks/use-delete-description';
import { HERO_FLOOR, useFitText } from '@/mobile/hooks/use-fit-text';
import { useBankTransactionMutations } from '@/hooks/use-bank-account-mutations';
import { useLedgerMutations } from '@/mobile/hooks/use-ledger-mutations';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useRetained } from '@/mobile/hooks/use-retained';
import { signedAmountText } from '@/mobile/lib/signed-amount';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { PlanPickerSheet } from '@/mobile/sheets/PlanPickerSheet';

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
 * The transaction sheet (spec §8.4, README §13, audit D11): the head says who, when and how much
 * once — the counterparty as the title, the day in words under it, the amount and the payment's
 * note below — and the rows hold only what can be changed: the category (a row that opens the
 * picker over this sheet), the spending plan (not for income, unless one is set), and what the
 * payment counts as (Automaticky or one of the roles, with what the automatic choice comes to and
 * why, in plain words). The technical details fold under "Podrobnosti", each shown in full and
 * copied with a tap; then Smazat as a text danger action, which asks first in a confirm sheet.
 * Changes save as they are made, so the sheet has no footer: "Hotovo" in the head closes it (C7).
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
  const { setFlowKind } = useLedgerMutations();
  const { deleteTransaction } = useBankTransactionMutations();
  const describeDelete = useDeleteDescription();
  const shown = useRetained(tx);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [planOpen, setPlanOpen] = useState(false);
  const planIndex = useSpendingPlanIndex(open);
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
  const plan = planIndex.data?.find((s) => s.plan.id === shown.spendingPlanId)?.plan;
  // A tagged payment whose plan is not known (yet) is not "Bez plánu"
  const planText = plan
    ? plan.name
    : shown.spendingPlanId === null
      ? t('transaction.noPlan')
      : planIndex.isLoading
        ? '…'
        : '—';

  const flowOptions = FLOW_CHOICES.map((choice) => ({
    value: choice,
    // "Automaticky" short, so the four choices fit at 360 in every language
    label: choice === 'auto' ? t('transaction.auto') : tb(`readiness.flow.${choice}`),
  }));
  // What the automatic choice comes to and why, in plain words ("Příjem · podle kategorie")
  const meaning =
    auto && flow
      ? `${tb(`readiness.flow.${flow.effectiveFlowKind}`)} · ${t(`transaction.by.${flow.flowSource}`)}`
      : undefined;
  // A spending plan is for spending: income has no row for it, unless a plan is set (to take it off)
  const effectiveKind =
    shown.flowKind ?? flow?.effectiveFlowKind ?? (shown.type === 'credit' ? 'income' : 'expense');
  const showPlan = effectiveKind !== 'income' || shown.spendingPlanId !== null;
  const changeFlow = (choice: FlowChoice) => {
    const flowKind = choice === 'auto' ? null : choice;
    setFlowKind.mutate({ id: shown.id, flowKind }, { onSuccess: () => onChange?.({ flowKind }) });
  };

  // The technical details: identifiers copy with a tap, the short facts are plain rows
  const technical: { label: string; value: string | null; copy?: boolean }[] = [
    {
      label: t('transaction.counterpartyAccount'),
      value: label.title === null ? null : label.subtitle,
      copy: true,
    },
    { label: tb('readiness.detail.bankId'), value: shown.transactionId, copy: true },
    {
      label: tb('readiness.detail.valueDate'),
      value: shown.valueDate === null ? null : fmt.day(shown.valueDate),
    },
    { label: tb('readiness.detail.file'), value: shown.importFileName, copy: true },
    {
      label: tb('readiness.detail.line'),
      value: shown.importLine === null ? null : String(shown.importLine),
    },
    { label: t('transaction.sourceLabel'), value: t(`transaction.source.${shown.dataSource}`) },
    {
      label: t('transaction.status'),
      value: shown.status === 'pending' ? tb('readiness.pending') : t('transaction.booked'),
    },
    { label: t('transaction.currency'), value: shown.currency },
  ];

  return (
    <>
      <Sheet
        full
        open={open}
        onOpenChange={changeOpen}
        title={title}
        description={fmt.day(shown.bookingDate, { day: 'numeric', month: 'long', year: 'numeric' })}
      >
        <Amount tx={shown} />
        {note.length > 0 && (
          <p className="mx-1 mb-2 mt-0 hyphens-auto whitespace-pre-line break-words text-m-body text-ink-3">
            {note.join('\n')}
          </p>
        )}
        <div className="divide-y divide-line-soft">
          <KeyValueRow
            label={t('transaction.category')}
            value={category ? categoryName(category) : t('account.ledger.noCategory')}
            onClick={onPickCategory}
          />
          {showPlan && (
            <KeyValueRow
              label={t('transaction.plan')}
              value={
                plan ? (
                  <span className="inline-flex max-w-full items-center justify-end gap-2">
                    <PlanChip plan={plan} size="sm" />
                    <span className="truncate">{plan.name}</span>
                  </span>
                ) : (
                  planText
                )
              }
              ariaLabel={`${t('transaction.plan')}, ${planText}`}
              onClick={() => setPlanOpen(true)}
            />
          )}
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
            {technical.flatMap(({ label: name, value, copy }) => {
              if (value === null || value === '') return [];
              return copy
                ? [<CopyRow key={name} label={name} value={value} />]
                : [<KeyValueRow key={name} label={name} value={value} />];
            })}
          </div>
        )}
        <button
          type="button"
          onClick={() => setConfirmOpen(true)}
          className="mt-2 flex min-h-11 w-full items-center px-1 py-2 text-left text-m-body font-semibold text-loss [-webkit-touch-callout:none] active:bg-well focus-visible:outline-none focus-visible:shadow-focus"
        >
          {tb('transaction.delete')}
        </button>
      </Sheet>
      <PlanPickerSheet
        tx={shown}
        open={planOpen}
        onOpenChange={setPlanOpen}
        onAssigned={(spendingPlanId) => onChange?.({ spendingPlanId })}
      />
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
 * A technical detail shown in full (an ID, a file name wraps anywhere rather than end in an
 * ellipsis) whose row copies it with a tap (audit D11): the label above, the value under it, the
 * copy icon at the end, a toast to say it is on the clipboard.
 */
function CopyRow({ label, value }: { label: string; value: string }) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      toast(t('transaction.copied', { label }));
    } catch {
      // No clipboard (or no permission to write to it): the value is still on screen in full
      toast.error(tc('status.error'));
    }
  };
  return (
    <button
      type="button"
      onClick={() => void copy()}
      aria-label={`${label}, ${value}. ${tc('buttons.copy')}`}
      className="flex min-h-11 w-full items-center gap-3 px-1 py-2 text-left [-webkit-touch-callout:none] active:bg-well focus-visible:outline-none focus-visible:shadow-focus"
    >
      <span className="min-w-0 flex-1">
        <span className="block text-m-sub text-ink-3">{label}</span>
        <span className="mt-0.5 block break-all text-m-body text-ink num">{value}</span>
      </span>
      <Copy aria-hidden className="size-4 flex-none text-ink-4" strokeWidth={1.75} />
    </button>
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
            : { fontSize: `clamp(${HERO_FLOOR}, ${fit}px, var(--m-fs-display))` }
        }
      >
        <span>{text}</span>
      </div>
    </div>
  );
}
