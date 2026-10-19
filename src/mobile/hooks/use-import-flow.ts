import { useEffect, useMemo, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { open as openFileDialog } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type {
  BankAccountWithInstitution,
  CsvCategoryOverride,
  CsvImportResult,
  CsvPreviewResult,
  StatementImportOptionsInput,
  StatementInspection,
  DateOrder,
} from '@shared/schema';
import {
  buildImportConfig,
  fileNameFromPath,
  mappingFromInspection,
  statementDateOrder,
  statementImportBlocked,
  summarizePreview,
} from '@/components/bank-accounts/csv-import/import-config';
import {
  useCsvImportMutation,
  useStatementImportMutation,
} from '@/components/bank-accounts/csv-import/use-csv-import-mutations';
import { useCsvPreview } from '@/components/bank-accounts/csv-import/use-csv-preview';
import { useStatementPreview } from '@/components/bank-accounts/csv-import/use-statement-preview';
import { useAuth } from '@/hooks/use-auth';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { bankAccountsApi, receivedFilesApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { utcDayFloor } from '@/utils/chart-axis';
import { dialogPathToFsPath } from '@/mobile/lib/export-file';
import { guideForAccount, guidesList } from '@/mobile/lib/export-guide';
import {
  PREVIEW_ROWS,
  chooseAccount,
  csvReadable,
  fileKind,
  isOpenAccount,
  pickedFileStillWanted,
  statementNeedsStage,
  type ImportRequest,
  type ImportStage,
} from '@/mobile/lib/import-flow';

/** Why a file ended in the `unreadable` stage; `error` carries the translated message as its detail. */
export interface Unreadable {
  reason: 'pdf' | 'image' | 'notStatement' | 'error';
  detail?: string;
}

const PRESETS_KEY = ['csv-presets'] as const;
const ACCOUNTS_KEY = ['bank-accounts'] as const;

/** The copies iOS makes of a shared or picked file are plaintext in the app's Inbox: only there. */
const discardsCopies = () => import.meta.env.TAURI_ENV_PLATFORM === 'ios';

/** Deletes a received copy, best effort: a refusal means the path was not one (a desktop dev path). */
function discardPath(copy: string) {
  if (discardsCopies()) void receivedFilesApi.discard(copy).catch(() => undefined);
}

/**
 * The import sheet's state machine (spec §8.6), one `ImportStage` at a time:
 *
 *   pick → reading → [statement] → [account] → preview → importing → done
 *                  ↘ unreadable (a PDF, a photo, a file that is no statement, a read error)
 *
 * It reuses the desktop's parsers, previews and write commands as they are; this hook only walks
 * the file through them and keeps what the stages show. `reading` is where a file is told apart:
 * `inspectStatementFile` first (CAMT.053, MT940, OFX, QIF), `parseCsvFile` when that says null;
 * the account is asked only when it cannot be told (`chooseAccount`). A read is dropped when the
 * person started another or closed the sheet (`reads`). The copy a share or the picker made is
 * discarded when the sheet closes, is replaced by another file, or is unmounted (iOS only).
 */
export function useImportFlow(request: ImportRequest, open: boolean) {
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const { user } = useAuth();
  // Inspect, preview and import decode a legacy-encoded file the same way
  const country = user?.country ?? undefined;
  const { accounts } = useBankAccounts();
  const [todayDay] = useState(() => utcDayFloor(Date.now() / 1000));

  const [stage, setStage] = useState<ImportStage>(request.path ? 'reading' : 'pick');
  const [path, setPath] = useState<string | null>(request.path ?? null);
  const [unreadable, setUnreadable] = useState<Unreadable | null>(null);
  const [statement, setStatement] = useState<StatementInspection | null>(null);
  /** The account `statement` was read for: its account check is relative to it. */
  const [inspectedFor, setInspectedFor] = useState<string | null>(null);
  const [statementIndex, setStatementIndex] = useState(0);
  const [pickedDateOrder, setPickedDateOrder] = useState<DateOrder | null>(null);
  const [csv, setCsv] = useState<CsvPreviewResult | null>(null);
  const [targetId, setTargetId] = useState<string | null>(request.accountId ?? null);
  const [accountConfirmed, setAccountConfirmed] = useState(false);
  const [overrides, setOverrides] = useState<CsvCategoryOverride[]>([]);
  const [result, setResult] = useState<CsvImportResult | null>(null);
  /** The account `result` was written to, fixed when the write started. */
  const [importedId, setImportedId] = useState<string | null>(null);
  /** The statement is being read again for another target account. */
  const [switching, setSwitching] = useState(false);

  /** Bumped per read; an answer that is no longer the latest is dropped. */
  const reads = useRef(0);
  /** The file the sheet works on, while it is a copy to discard. */
  const owned = useRef<string | null>(null);

  const kind = statement ? 'statement' : csv ? 'csv' : null;
  const summary = statement?.statements.find((s) => s.index === statementIndex);
  const dateOrder = statement
    ? statementDateOrder(statement, pickedDateOrder)
    : { dateOrder: null, confirmed: true };
  const openAccounts = useMemo(
    () => accounts.filter((account) => isOpenAccount(account, todayDay)),
    [accounts, todayDay]
  );
  const target = accounts.find((account) => account.id === targetId);

  const { data: presets = [] } = useQuery({
    queryKey: PRESETS_KEY,
    queryFn: () => bankAccountsApi.getCsvPresets(),
    staleTime: Infinity,
    enabled: open,
  });
  const preset = csv?.detectedPresetId
    ? presets.find((candidate) => candidate.id === csv.detectedPresetId)
    : undefined;
  // The export guides (spec §8.6, rung 2): the account's bank on the picker, every bank when a file cannot be read
  const guides = useMemo(() => guidesList(presets), [presets]);
  const requestedAccount = accounts.find((account) => account.id === request.accountId);
  const accountGuide = useMemo(
    () => guideForAccount(presets, requestedAccount),
    [presets, requestedAccount]
  );

  // ── The files the sheet holds on to ─────────────────────────────────────────

  const discardOwned = () => {
    const copy = owned.current;
    owned.current = null;
    if (copy) discardPath(copy);
  };
  const adopt = (filePath: string) => {
    if (owned.current !== null && owned.current !== filePath) discardOwned();
    owned.current = filePath;
  };

  // ── Reading a file ──────────────────────────────────────────────────────────

  const fetchAccounts = () =>
    queryClient.fetchQuery({
      queryKey: ACCOUNTS_KEY,
      queryFn: () => bankAccountsApi.getAll(),
      staleTime: 10_000,
    });

  const giveUp = (why: Unreadable) => {
    setUnreadable(why);
    setStage('unreadable');
  };

  /**
   * The statement `inspection` for `index` goes to the account the file says it belongs to (or the
   * one the entry point named); the file is read again for that account when it differs from the
   * one it was read for, so the account check holds for the target.
   */
  const settleStatement = async (args: {
    inspection: StatementInspection;
    index: number;
    anchor: string;
    filePath: string;
    all: readonly BankAccountWithInstitution[];
    preselected?: string;
    read: number;
  }) => {
    const { inspection, index, anchor, filePath, all, preselected, read } = args;
    const picked = inspection.statements.find((s) => s.index === index);
    const choice = chooseAccount({
      preselected,
      statement: picked && {
        match: picked.accountMatch,
        matchedAccountId: picked.matchedAccountId,
      },
      accounts: all,
      todayDay,
    });
    if (choice.accountId === null) {
      setStage('account');
      return;
    }
    setTargetId(choice.accountId);
    if (choice.accountId !== anchor) {
      const next = await bankAccountsApi.inspectStatementFile(choice.accountId, filePath, country);
      if (read !== reads.current) return;
      if (next === null) return giveUp({ reason: 'notStatement' });
      setStatement(next);
      setInspectedFor(choice.accountId);
      if (!next.statements.some((s) => s.index === index)) {
        setStatementIndex(next.defaultStatementIndex);
      }
    }
    setStage('preview');
  };

  /**
   * Reads `filePath` from the start. `forced` is an account that was just created for it: the
   * file's account check is anchored there.
   */
  const read = async (filePath: string, forced?: string) => {
    const mine = ++reads.current;
    adopt(filePath);
    setPath(filePath);
    setUnreadable(null);
    setStatement(null);
    setInspectedFor(null);
    setStatementIndex(0);
    setPickedDateOrder(null);
    setCsv(null);
    setTargetId(null);
    setAccountConfirmed(false);
    setOverrides([]);
    setResult(null);
    setStage('reading');

    const named = fileKind(fileNameFromPath(filePath));
    if (named !== 'readable') return giveUp({ reason: named });
    try {
      const all = await fetchAccounts();
      if (mine !== reads.current) return;
      const preselected = forced ?? request.accountId;
      // The account only anchors the statement's account check; without any, the person adds one
      const anchor = preselected ?? all.find((account) => isOpenAccount(account, todayDay))?.id;
      if (!anchor) {
        setStage('account');
        return;
      }

      const inspected = await bankAccountsApi.inspectStatementFile(anchor, filePath, country);
      if (mine !== reads.current) return;
      if (inspected !== null) {
        setStatement(inspected);
        setInspectedFor(anchor);
        setStatementIndex(inspected.defaultStatementIndex);
        if (statementNeedsStage(inspected)) {
          setStage('statement');
          return;
        }
        await settleStatement({
          inspection: inspected,
          index: inspected.defaultStatementIndex,
          anchor,
          filePath,
          all,
          preselected,
          read: mine,
        });
        return;
      }

      const parsed = await bankAccountsApi.parseCsvFile(
        filePath,
        undefined, // always detect the delimiter; bank formats change
        undefined,
        undefined,
        undefined,
        undefined,
        country
      );
      if (mine !== reads.current) return;
      if (!csvReadable(parsed)) return giveUp({ reason: 'notStatement' });
      setCsv(parsed);
      const known = await queryClient.fetchQuery({
        queryKey: PRESETS_KEY,
        queryFn: () => bankAccountsApi.getCsvPresets(),
        staleTime: Infinity,
      });
      if (mine !== reads.current) return;
      const choice = chooseAccount({
        preselected,
        presetInstitutionId:
          known.find((candidate) => candidate.id === parsed.detectedPresetId)?.institutionId ??
          null,
        accounts: all,
        todayDay,
      });
      if (choice.accountId === null) {
        setStage('account');
        return;
      }
      setTargetId(choice.accountId);
      setStage('preview');
    } catch (error) {
      if (mine !== reads.current) return;
      giveUp({ reason: 'error', detail: translateApiError(error as Error, tc) });
    }
  };

  // A file that is already at hand (a shared one) is read once, when the sheet opens
  const readRef = useRef(read);
  useEffect(() => {
    readRef.current = read;
  });
  const openRef = useRef(open);
  useEffect(() => {
    openRef.current = open;
  });
  const started = useRef(false);
  useEffect(() => {
    if (started.current || !request.path) return;
    started.current = true;
    void readRef.current(request.path);
  }, [request.path]);

  // The sheet is gone (closed, replaced by another import, locked): drop the reads, discard the copy
  useEffect(
    () => () => {
      reads.current++;
      discardOwned();
    },
    // `discardOwned` only touches refs, so the cleanup is the same on every render
    []
  );

  /** The Files picker: no type filter — a bank's export may be `.STA`, `.940` or have no extension. */
  const chooseFile = async () => {
    const askedAt = reads.current;
    const stillWanted = () =>
      pickedFileStillWanted({ open: openRef.current, askedAt, now: reads.current });
    try {
      const selected = await openFileDialog({ multiple: false, directory: false });
      if (!selected) return;
      const picked = dialogPathToFsPath(selected);
      // The sheet went away while the picker was up (a lock, Android back): iOS has copied the
      // file into the app's Inbox, so nothing else will ever discard it
      if (!stillWanted()) return discardPath(picked);
      await read(picked);
    } catch (error) {
      if (!stillWanted()) return;
      giveUp({ reason: 'error', detail: translateApiError(error as Error, tc) });
    }
  };

  // ── Stages that ask something ───────────────────────────────────────────────

  /** The statement stage is answered: on to the account. */
  const continueFromStatement = async () => {
    if (!statement || !path || !inspectedFor) return;
    const mine = ++reads.current;
    // Confirms the order the dates are read in (the detected one, unless the person picked another)
    if (dateOrder.dateOrder) setPickedDateOrder(dateOrder.dateOrder);
    setStage('reading');
    try {
      const all = await fetchAccounts();
      if (mine !== reads.current) return;
      await settleStatement({
        inspection: statement,
        index: statementIndex,
        anchor: inspectedFor,
        filePath: path,
        all,
        preselected: request.accountId,
        read: mine,
      });
    } catch (error) {
      if (mine !== reads.current) return;
      giveUp({ reason: 'error', detail: translateApiError(error as Error, tc) });
    }
  };

  /**
   * An account was chosen (or just created, or is the one the statement belongs to): it becomes
   * the target, a statement is read again for it, and the preview opens.
   */
  const selectAccount = async (accountId: string) => {
    if (!path) return;
    // Nothing was read yet: there was no account to anchor the file's account check
    if (!statement && !csv) {
      await read(path, accountId);
      return;
    }
    const previous = targetId;
    setTargetId(accountId);
    setAccountConfirmed(false);
    if (statement && inspectedFor !== accountId) {
      const mine = ++reads.current;
      // From the preview the sheet stays where it is (its button waits); from the account list it reads
      if (stage !== 'preview') setStage('reading');
      setSwitching(true);
      try {
        const next = await bankAccountsApi.inspectStatementFile(accountId, path, country);
        if (mine !== reads.current) return;
        if (next === null) return giveUp({ reason: 'notStatement' });
        setStatement(next);
        setInspectedFor(accountId);
        if (!next.statements.some((s) => s.index === statementIndex)) {
          setStatementIndex(next.defaultStatementIndex);
        }
      } catch (error) {
        if (mine !== reads.current) return;
        // What is on screen still speaks of the previous target
        setTargetId(previous);
        if (stage !== 'preview') setStage('account');
        toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
        return;
      } finally {
        if (mine === reads.current) setSwitching(false);
      }
    }
    setStage('preview');
  };

  const setOverride = (line: number, categoryId: string | null) =>
    setOverrides((current) => {
      const rest = current.filter((o) => o.line !== line);
      return categoryId ? [...rest, { line, categoryId }] : rest;
    });

  // ── Preview and import ──────────────────────────────────────────────────────

  const orderValue = dateOrder.dateOrder;
  const statementOptions = useMemo<StatementImportOptionsInput | null>(
    () =>
      statement
        ? {
            statementIndex,
            ...(orderValue ? { dateOrder: orderValue } : {}),
            importAnywayLines: [],
            categoryOverrides: [...overrides].sort((a, b) => a.line - b.line),
          }
        : null,
    [statement, statementIndex, orderValue, overrides]
  );
  const csvConfig = useMemo(
    () => (csv ? buildImportConfig(mappingFromInspection(csv), csv, 0, [], overrides) : null),
    [csv, overrides]
  );
  // New objects on every render would restart the previews' debounce each time
  const statementPreviewOptions = useMemo(
    () =>
      statementOptions
        ? { ...statementOptions, previewOffset: 0, previewLimit: PREVIEW_ROWS }
        : null,
    [statementOptions]
  );
  const csvPreviewConfig = useMemo(
    () => (csvConfig ? { ...csvConfig, previewOffset: 0, previewLimit: PREVIEW_ROWS } : null),
    [csvConfig]
  );
  const previewing = open && (stage === 'preview' || stage === 'importing') && targetId !== null;
  const statementPreview = useStatementPreview({
    accountId: targetId ?? '',
    filePath: path ?? '',
    options: statementPreviewOptions,
    country,
    enabled: previewing && kind === 'statement' && !switching,
  });
  const csvPreview = useCsvPreview({
    accountId: targetId ?? '',
    filePath: path ?? '',
    config: csvPreviewConfig,
    enabled: previewing && kind === 'csv',
  });
  const {
    preview,
    error: previewError,
    isUpdating,
    isLoadingFirst,
  } = kind === 'statement' ? statementPreview : csvPreview;
  const counts = preview ? summarizePreview(preview, []) : null;

  /** A statement of another account (or of none) is imported only once the person said so (D9). */
  const confirmRequired =
    kind === 'statement' && statementImportBlocked(summary, false, inspectedFor === targetId);
  const blocked =
    kind === 'statement' &&
    (statementImportBlocked(summary, accountConfirmed, inspectedFor === targetId) ||
      !dateOrder.confirmed);
  const canImport =
    stage === 'preview' &&
    preview !== undefined &&
    counts !== null &&
    !isUpdating &&
    !switching &&
    !previewError &&
    !blocked &&
    (counts.willImport > 0 || counts.willPromote > 0) &&
    (kind === 'statement' ? statementOptions !== null : csvConfig !== null);

  const csvMutation = useCsvImportMutation(targetId ?? '');
  const statementMutation = useStatementImportMutation();

  const runImport = async () => {
    if (!canImport || targetId === null || path === null) return;
    // The account this write goes to; the done step acts on it
    const writeAccountId = targetId;
    setStage('importing');
    try {
      // Preview and import get the same options, so what the person saw is what is written
      const written =
        kind === 'statement' && statementOptions
          ? await statementMutation.mutateAsync({
              accountId: writeAccountId,
              filePath: path,
              options: statementOptions,
              country,
            })
          : await csvMutation.mutateAsync({ filePath: path, config: csvConfig! });
      setResult(written);
      setImportedId(writeAccountId);
      setStage('done');
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
      setStage('preview');
    }
  };

  /** The sheet is closing: drop what is being read, discard the copy of the file. */
  const finish = () => {
    reads.current++;
    discardOwned();
  };

  return {
    stage,
    path,
    fileName: path ? fileNameFromPath(path) : '',
    unreadable,
    kind,
    statement,
    summary,
    statementIndex,
    setStatementIndex,
    dateOrder,
    pickDateOrder: setPickedDateOrder,
    csv,
    preset,
    guides,
    accountGuide,
    openAccounts,
    accounts,
    target,
    targetId,
    preview,
    previewError,
    previewUpdating: isUpdating || switching,
    previewLoading: isLoadingFirst,
    counts,
    overrides,
    setOverride,
    accountConfirmed,
    setAccountConfirmed,
    confirmRequired,
    canImport,
    switching,
    result,
    importedId,
    chooseFile,
    continueFromStatement,
    selectAccount,
    runImport,
    finish,
  };
}

export type ImportFlow = ReturnType<typeof useImportFlow>;
