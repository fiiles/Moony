import { useCallback, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useSearch } from 'wouter';
import { useQuery } from '@tanstack/react-query';
import { CircleCheck } from 'lucide-react';
import type {
  BankTransaction,
  ChangedRow,
  SuggestionSource,
  TransactionCategory,
} from '@shared/schema';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useCategories } from '@/hooks/use-categories';
import { cashflowApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { EmptyState } from '@/components/common/EmptyState';
import { Button } from '@/components/ui/button';
import { ProgressBar } from '@/mobile/components/cashflow/ProgressBar';
import { EntityRows } from '@/mobile/components/EntityRow';
import { PickRunner, type PickRun } from '@/mobile/components/queue/PickRunner';
import { QueueSkeleton } from '@/mobile/components/queue/QueueSkeleton';
import { QueueRow } from '@/mobile/components/QueueRow';
import { useBack } from '@/mobile/hooks/use-back';
import {
  useCategorizeQueue,
  useQueueSnapshot,
  useRecentCategoryIds,
  useRestoreCategories,
  useSnoozeCategorization,
} from '@/mobile/hooks/use-categorize-queue';
import { useLedgerMutations } from '@/mobile/hooks/use-ledger-mutations';
import { useRetained } from '@/mobile/hooks/use-retained';
import { useLastImports } from '@/mobile/hooks/use-account-upkeep';
import {
  SNOOZE_DAYS,
  batchOffered,
  doneCategories,
  effectiveOpenIds,
  openCount,
  queueProgress,
  rowState,
  type LocalOutcome,
} from '@/mobile/lib/categorize-queue';
import { shortDayOptions } from '@/mobile/lib/format-day';
import { selectionTick } from '@/mobile/lib/haptics';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { CategoryPickerSheet } from '@/mobile/sheets/CategoryPickerSheet';
import { TransactionSheet } from '@/mobile/sheets/TransactionSheet';

/** Route entry: another account (`?account=`) is another queue, so its snapshot and progress start over. */
export default function CategorizeQueue() {
  const search = useSearch();
  const account = new URLSearchParams(search).get('account') ?? undefined;
  return <QueuePage key={account ?? 'all'} accountId={account} />;
}

/**
 * Zařazení plateb (spec §8.5, prototype frame 05, `/finance/uncategorized`): the payments without a
 * category, newest first, each with the category the app would give it as a pill — a tap on it
 * accepts it (a haptic tick, the row turns over) — and "Jiná…" for the picker. The rows are a
 * snapshot taken when the page opens, so nothing jumps under the finger: a row that is dealt with
 * stays where it is, shows what it became and counts in "3 z 12". After an accept that took other
 * payments along the row opens once with a switch that gives them back. "Nechat" (a swipe, the
 * pill, the menu) holds a payment back for 30 days. When nothing is left: "Všechno zařazeno".
 */
function QueuePage({ accountId }: { accountId: string | undefined }) {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const back = useBack(routes.finance);
  const [, setLocation] = useLocation();
  const { categories, isLoading: categoriesLoading } = useCategories();
  const { accounts } = useBankAccounts();
  const { acceptSuggestion } = useLedgerMutations({ quiet: true });
  const snooze = useSnoozeCategorization();
  const restoreCategories = useRestoreCategories();

  const queue = useCategorizeQueue(accountId);
  // What this page did to a row. It counts before the backend's answer, which lags a write until
  // its refresh settles, and it is what remembers a row's category once the row has left the answer.
  const [local, setLocal] = useState<ReadonlyMap<string, LocalOutcome>>(new Map());
  const snapshot = useQueueSnapshot(queue.data, !queue.isFetching, local);
  // Payments deleted in the details sheet: not "done", simply gone
  const [removed, setRemoved] = useState<ReadonlySet<string>>(new Set());
  // What the details sheet changed on a row (the meaning of the movement): the snapshot's copy of the
  // row follows, as the account detail's `patchTx` does, so the sheet shows it and reopens with it
  const [patches, setPatches] = useState<ReadonlyMap<string, Partial<BankTransaction>>>(new Map());
  const rows = useMemo(
    () =>
      (snapshot ?? [])
        .filter((row) => !removed.has(row.transaction.id))
        .map((row) => {
          const patch = patches.get(row.transaction.id);
          return patch ? { ...row, transaction: { ...row.transaction, ...patch } } : row;
        }),
    [snapshot, removed, patches]
  );

  // The switches of the expansions that were turned off (their payments are restored)
  const [restored, setRestored] = useState<ReadonlySet<string>>(new Set());
  const openIds = useMemo(() => effectiveOpenIds(queue.data, local), [queue.data, local]);
  const done = useMemo(() => doneCategories(local, restored), [local, restored]);
  const categoryById = useMemo(
    () => new Map<string, TransactionCategory>(categories.map((c) => [c.id, c])),
    [categories]
  );
  const progress = queueProgress(
    rows.map((row) => row.transaction.id),
    openIds
  );
  const open = openCount(queue.data, local);

  // Accepted engine suggestions being saved; a row is only accepted once
  const [runs, setRuns] = useState<PickRun[]>([]);
  const accepting = useRef(new Set<string>());
  const [detailsFor, setDetailsFor] = useState<BankTransaction | null>(null);
  const [pickFor, setPickFor] = useState<BankTransaction | null>(null);
  // The sheets keep their row while they slide away, and so does what is read for it
  const detailsRow = useRetained(detailsFor);
  const flow = useQuery({
    queryKey: ['effective-transaction-flows', [detailsRow?.id]],
    queryFn: () => cashflowApi.getEffectiveFlows([detailsRow?.id as string]),
    enabled: detailsRow !== null,
  });
  const pickerRow = useRetained(pickFor);
  const recentIds = useRecentCategoryIds(pickerRow?.bankAccountId);

  const setOutcome = (id: string, outcome: LocalOutcome) =>
    setLocal((current) => new Map(current).set(id, outcome));
  const clearOutcome = (id: string) =>
    setLocal((current) => {
      const next = new Map(current);
      next.delete(id);
      return next;
    });
  const categorized = (id: string, categoryId: string) =>
    setOutcome(id, { kind: 'categorized', categoryId, batch: [] });
  // The learning reports after the pick has been recorded (`PickOptions.onApplied`)
  const takeBatch = useCallback((id: string, changed: ChangedRow[]) => {
    if (changed.length === 0) return;
    setLocal((current) => {
      const outcome = current.get(id);
      if (outcome?.kind !== 'categorized') return current;
      return new Map(current).set(id, { ...outcome, batch: changed });
    });
  }, []);

  /** The dark pill: an MCP suggestion is accepted as it is, an engine one is picked and remembered. */
  const accept = (tx: BankTransaction, categoryId: string, source: SuggestionSource) => {
    if (local.has(tx.id) || accepting.current.has(tx.id)) return;
    accepting.current.add(tx.id);
    // The row turns over at the tap; a failed write (it says so itself) gives it back
    categorized(tx.id, categoryId);
    selectionTick();
    if (source === 'mcp') {
      acceptSuggestion
        .mutateAsync(tx.id)
        .catch(() => clearOutcome(tx.id))
        .finally(() => accepting.current.delete(tx.id));
    } else {
      setRuns((current) => [...current, { tx, categoryId }]);
    }
  };
  const finishRun = useCallback((txId: string, saved: boolean) => {
    accepting.current.delete(txId);
    if (!saved) clearOutcome(txId);
    setRuns((current) => current.filter((run) => run.tx.id !== txId));
  }, []);

  /** "Nechat": 30 days out of the queue. */
  const keep = (tx: BankTransaction) => {
    if (local.has(tx.id)) return;
    setOutcome(tx.id, { kind: 'left' });
    snooze.mutateAsync([tx.id]).catch(() => clearOutcome(tx.id));
  };

  const restore = (id: string, batch: ChangedRow[]) => {
    setRestored((current) => new Set(current).add(id));
    void restoreCategories(batch).then((ok) => {
      if (ok) return;
      setRestored((current) => {
        const next = new Set(current);
        next.delete(id);
        return next;
      });
    });
  };

  // A done row's picker opens on the category it has now
  const withCategory = (tx: BankTransaction): BankTransaction => {
    const categoryId = done.get(tx.id);
    return categoryId ? { ...tx, categoryId } : tx;
  };

  // The line under the nav bar: the account and the statement the payments came from, or how many accounts
  const accountIds = useMemo(
    () => [...new Set(rows.map((row) => row.transaction.bankAccountId))],
    [rows]
  );
  const named = useMemo(
    () => accounts.filter((account) => accountIds.includes(account.id)),
    [accounts, accountIds]
  );
  const { lastImportByAccount } = useLastImports(named);
  let source: string | null = null;
  if (accountIds.length > 1) {
    source = t('upkeep.accounts', { count: accountIds.length });
  } else if (named.length === 1) {
    const last = lastImportByAccount[named[0].id];
    source =
      last != null
        ? t('upkeep.uncategorizedFrom', {
            account: named[0].name,
            date: fmt.day(last, shortDayOptions(last)),
          })
        : named[0].name;
  }
  const lead = [source, progress.handled > 0 && t('queue.handled', { count: progress.handled })]
    .filter(Boolean)
    .join(' · ');

  // The pills name categories: until they are known a suggestion would show as "Vybrat kategorii…"
  const ready = snapshot !== null && !categoriesLoading;
  const finished = ready && open === 0;
  // The last accept may have taken other payments along: its expansion is the only way back, so the
  // done rows stay under the empty state while any switch is still on
  const offered = batchOffered(local, restored);

  return (
    <Screen
      title={t('finance.uncategorized')}
      leading={<NavTextButton label={t('tabs.finance')} onClick={back} chevron />}
    >
      {!ready ? (
        queue.isError ? (
          <div className="mx-0.5 py-6 text-center">
            <p className="mb-3 mt-0 text-m-body text-ink-3">{t('account.ledger.loadError')}</p>
            <Button
              variant="secondary"
              className="h-11 rounded-[10px] px-4 text-m-row"
              onClick={() => void queue.refetch()}
            >
              {t('account.ledger.retry')}
            </Button>
          </div>
        ) : (
          <QueueSkeleton />
        )
      ) : (
        <>
          {finished ? (
            <EmptyState
              bare
              icon={<CircleCheck />}
              title={t('queue.empty.title')}
              description={
                queue.data && queue.data.snoozed > 0
                  ? t('queue.empty.snoozed', { count: queue.data.snoozed })
                  : t('queue.empty.hint')
              }
              className={offered ? 'px-5 py-8' : 'px-5 py-12'}
              action={
                <Button
                  className="h-12 w-full rounded-[10px] px-6 text-m-row"
                  onClick={() => setLocation(routes.cashflow)}
                >
                  {t('queue.showCashflow')}
                </Button>
              }
            />
          ) : (
            <>
              {lead && (
                <p className="mx-0.5 mb-1 mt-0.5 text-[13px] font-450 leading-[1.4] text-ink-3">
                  {lead}
                </p>
              )}
              <h2 className="mx-0.5 mb-2.5 mt-0 text-m-title text-ink">
                {t('upkeep.uncategorized', { count: open })}
              </h2>
              <div className="mx-0.5 mb-3 flex items-center gap-2.5 text-m-caption text-ink-3">
                <div className="min-w-0 flex-1">
                  <ProgressBar
                    value={progress.total > 0 ? progress.handled / progress.total : 0}
                    tone="limit"
                  />
                </div>
                <span className="flex-none whitespace-nowrap num">
                  {t('queue.progress', { done: progress.handled, total: progress.total })}
                </span>
              </div>
            </>
          )}
          {(!finished || offered) && (
            <EntityRows>
              {rows.map((row) => {
                const id = row.transaction.id;
                const outcome = local.get(id);
                const batch = outcome?.kind === 'categorized' ? outcome.batch : [];
                const { suggestion } = row;
                return (
                  <QueueRow
                    key={id}
                    row={row}
                    state={rowState(id, openIds, local)}
                    suggested={suggestion ? categoryById.get(suggestion.categoryId) : undefined}
                    doneCategory={categoryById.get(done.get(id) ?? '')}
                    expansion={
                      batch.length > 0
                        ? {
                            count: batch.length,
                            restored: restored.has(id),
                            onRestore: () => restore(id, batch),
                          }
                        : undefined
                    }
                    onAccept={() =>
                      suggestion &&
                      accept(row.transaction, suggestion.categoryId, suggestion.source)
                    }
                    onPick={() => setPickFor(withCategory(row.transaction))}
                    onSnooze={() => keep(row.transaction)}
                    onDetails={() => setDetailsFor(row.transaction)}
                  />
                );
              })}
            </EntityRows>
          )}
          {!finished && (
            <p className="mx-1 mb-0 mt-0 text-m-caption text-ink-3">
              {t('queue.caption', { keep: t('queue.keep'), days: SNOOZE_DAYS })}
            </p>
          )}
        </>
      )}

      {runs.map((run) => (
        <PickRunner
          key={run.tx.id}
          run={run}
          categories={categories}
          onApplied={(changed) => takeBatch(run.tx.id, changed)}
          onDone={finishRun}
        />
      ))}
      <TransactionSheet
        tx={detailsFor}
        open={detailsFor !== null}
        onOpenChange={(next) => !next && setDetailsFor(null)}
        category={categoryById.get(done.get(detailsRow?.id ?? '') ?? detailsRow?.categoryId ?? '')}
        flow={flow.data?.[0]}
        onPickCategory={() => detailsFor && setPickFor(withCategory(detailsFor))}
        onChange={(patch) => {
          if (!detailsFor) return;
          const id = detailsFor.id;
          setDetailsFor((current) => (current?.id === id ? { ...current, ...patch } : current));
          setPatches((current) => new Map(current).set(id, { ...current.get(id), ...patch }));
        }}
        onDeleted={() => detailsFor && setRemoved((current) => new Set(current).add(detailsFor.id))}
      />
      <CategoryPickerSheet
        tx={pickFor}
        open={pickFor !== null}
        onOpenChange={(next) => !next && setPickFor(null)}
        categories={categories}
        recentIds={recentIds}
        onChange={(patch) => {
          if (pickFor && typeof patch.categoryId === 'string') {
            categorized(pickFor.id, patch.categoryId);
          }
        }}
        onApplied={(changed) => pickFor && takeBatch(pickFor.id, changed)}
      />
    </Screen>
  );
}
