import { useMemo, useState, type MutableRefObject } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { bankAccountsApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { categoryDisplayName } from '@/utils/category-name';
import type {
  CsvCategoryOverride,
  DateOrder,
  StatementImportOptionsInput,
  StatementInspection,
  TransactionCategory,
} from '@shared/schema';
import { statementDateOrder, statementImportBlocked, type ImportKind } from './import-config';
import { matchSourceCategories, sourceCategoryMapping } from './source-categories';

/** One empty list, so the prefill (and the options) keep their identity until categories load. */
const NO_CATEGORIES: readonly TransactionCategory[] = [];

interface UseStatementSessionArgs {
  /** The page's account: the import's target until the user switches (D9). */
  accountId: string;
  /** The profile country: inspect, preview and import decode a legacy-encoded file the same way. */
  country: string | undefined;
  /** The file that is open; a switch reads it again for the new target. */
  filePath: string;
  /** The preview's picks (shared with the CSV path), sent with the statement's options. */
  importAnywayLines: readonly number[];
  overrides: readonly CsvCategoryOverride[];
  /** Moony's categories, for the prefill of the source-category panel (spec §8.2). */
  categories: readonly TransactionCategory[];
  /** Bumped per file read (CSV or statement); a response that is no longer the latest is dropped. */
  inspectRequest: MutableRefObject<number>;
  setIsInspecting: (inspecting: boolean) => void;
  /**
   * Forget the preview's picks: `duplicates` = the "import anyway" lines (they are relative to
   * the target account); `all` = those, the category picks and the page (entry ordinals belong
   * to the statement they were picked in).
   */
  clearPicks: (scope: 'duplicates' | 'all') => void;
  /** A statement was read from `path`: the dialog shows its preview. */
  onStatementOpened: (path: string) => void;
  /** `path` is not a statement: the CSV path reads it, unchanged. */
  onCsvFile: (path: string) => void;
}

/**
 * The statement side of the import dialog: what the open file is, the statement read from it
 * (for which account), the statement chosen in the picker, the account check (D9) with its
 * confirmation and the switch to the statement's own account, the QIF date order and source
 * categories (spec §6.4, §8.2), and the options a statement preview and import use.
 */
export function useStatementSession({
  accountId,
  country,
  filePath,
  importAnywayLines,
  overrides,
  categories,
  inspectRequest,
  setIsInspecting,
  clearPicks,
  onStatementOpened,
  onCsvFile,
}: UseStatementSessionArgs) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  // The account the import writes to: the page's, until the user switches to the
  // account the statement belongs to (D9). Derived, so a new page account applies at once.
  const [switchedAccountId, setTargetAccountId] = useState<string | null>(null);
  const targetAccountId = switchedAccountId ?? accountId;
  // What the open file is; null until one was read.
  const [kind, setKind] = useState<ImportKind | null>(null);
  const [inspection, setInspection] = useState<StatementInspection | null>(null);
  // The account `inspection` was read for: its account check is relative to it.
  const [inspectedAccountId, setInspectedAccountId] = useState<string | null>(null);
  const [statementIndex, setStatementIndex] = useState(0);
  // "Import into this account anyway" for a statement of another account (D9).
  const [accountConfirmed, setAccountConfirmed] = useState(false);
  // The date order the user picked or confirmed for a file whose dates read both ways.
  const [pickedDateOrder, setPickedDateOrder] = useState<DateOrder | null>(null);
  // Categories the user chose in the panel, by source path (null = leave to the rules).
  const [sourcePicks, setSourcePicks] = useState<Record<string, string | null>>({});

  /**
   * Reads the file as a statement first. Not a statement (null): the CSV path,
   * unchanged. A statement goes straight to its preview.
   */
  const open = async (path: string) => {
    const requestId = ++inspectRequest.current;
    setIsInspecting(true);
    try {
      const statement = await bankAccountsApi.inspectStatementFile(targetAccountId, path, country);
      if (requestId !== inspectRequest.current) return;
      if (statement == null) {
        setKind('csv');
        setInspection(null);
        onCsvFile(path);
        return;
      }
      setKind('statement');
      setInspection(statement);
      setInspectedAccountId(targetAccountId);
      setStatementIndex(statement.defaultStatementIndex);
      setAccountConfirmed(false);
      setPickedDateOrder(null);
      setSourcePicks({});
      clearPicks('all');
      onStatementOpened(path);
    } catch (e: unknown) {
      if (requestId !== inspectRequest.current) return;
      toast.error(tc('status.error'), { description: translateApiError(e as Error, tc) });
    } finally {
      if (requestId === inspectRequest.current) setIsInspecting(false);
    }
  };

  /**
   * "Import into {account}" (D9): the statement's own account becomes the target
   * and the file is read again for it, so the account check reads `selected`.
   */
  const switchAccount = async (matchedAccountId: string) => {
    const previousTarget = switchedAccountId;
    setTargetAccountId(matchedAccountId === accountId ? null : matchedAccountId);
    setAccountConfirmed(false);
    // Duplicates are another account's now; the categories picked still apply.
    clearPicks('duplicates');
    const requestId = ++inspectRequest.current;
    setIsInspecting(true);
    try {
      const statement = await bankAccountsApi.inspectStatementFile(
        matchedAccountId,
        filePath,
        country
      );
      if (requestId !== inspectRequest.current) return;
      if (statement == null) {
        // The file no longer reads as a statement: nothing was checked for the new target.
        setTargetAccountId(previousTarget);
        toast.error(tc('status.error'), { description: tc('validation.statementNotRecognized') });
        return;
      }
      setInspection(statement);
      setInspectedAccountId(matchedAccountId);
      if (!statement.statements.some((s) => s.index === statementIndex)) {
        setStatementIndex(statement.defaultStatementIndex);
      }
    } catch (e: unknown) {
      if (requestId !== inspectRequest.current) return;
      // The summary on screen still speaks of the previous target.
      setTargetAccountId(previousTarget);
      toast.error(tc('status.error'), { description: translateApiError(e as Error, tc) });
    } finally {
      if (requestId === inspectRequest.current) setIsInspecting(false);
    }
  };

  const changeStatement = (index: number) => {
    setStatementIndex(index);
    clearPicks('all');
    setAccountConfirmed(false);
  };

  /** No file: the next one starts as a file of unknown kind for the page's account. */
  const reset = () => {
    setKind(null);
    setInspection(null);
    setInspectedAccountId(null);
    setStatementIndex(0);
    setAccountConfirmed(false);
    setTargetAccountId(null);
    setPickedDateOrder(null);
    setSourcePicks({});
  };

  const pickSourceCategory = (source: string, categoryId: string | null) =>
    setSourcePicks((current) => ({ ...current, [source]: categoryId }));

  const summary = inspection?.statements.find((statement) => statement.index === statementIndex);
  // The account check (D9) is relative to the account the file was read for: after a
  // switch it holds for the new target once the file was read again for it (until then
  // the dialog's `isInspecting` holds the import back).
  const targetWasInspected = inspectedAccountId === targetAccountId;
  const { dateOrder, confirmed: dateOrderConfirmed } = inspection
    ? statementDateOrder(inspection, pickedDateOrder)
    : { dateOrder: null, confirmed: true };

  // `useCategories` hands a new empty list on every render until the categories are loaded;
  // new options on every render would restart the preview's debounce each time.
  const knownCategories = categories.length > 0 ? categories : NO_CATEGORIES;
  // The panel starts from the names that match (once per file: the sources of every
  // statement, so a pick holds when the user changes the statement); the user's picks win.
  const prefill = useMemo(
    () =>
      matchSourceCategories(
        [
          ...new Set(
            inspection?.statements.flatMap((s) => s.sourceCategories.map((c) => c.name)) ?? []
          ),
        ],
        knownCategories,
        (category) => categoryDisplayName(category, t)
      ),
    [inspection, knownCategories, t]
  );
  const sourceCategories = useMemo(() => ({ ...prefill, ...sourcePicks }), [prefill, sourcePicks]);
  const sourceCategoryMap = useMemo(
    () =>
      sourceCategoryMapping(
        summary?.sourceCategories.map((source) => source.name) ?? [],
        sourceCategories
      ),
    [summary, sourceCategories]
  );

  // What a statement import would do; null while no statement is open.
  const options = useMemo<StatementImportOptionsInput | null>(
    () =>
      kind === 'statement' && inspection
        ? {
            statementIndex,
            ...(dateOrder ? { dateOrder } : {}),
            importAnywayLines: [...importAnywayLines].sort((a, b) => a - b),
            categoryOverrides: [...overrides].sort((a, b) => a.line - b.line),
            ...(sourceCategoryMap.length > 0 ? { sourceCategoryMap } : {}),
          }
        : null,
    [kind, inspection, statementIndex, dateOrder, importAnywayLines, overrides, sourceCategoryMap]
  );

  return {
    kind,
    targetAccountId,
    inspection,
    /** The statement being previewed (`statementIndex`). */
    summary,
    accountConfirmed,
    setAccountConfirmed,
    /** The account check stops the import until `accountConfirmed` (D9). */
    confirmRequired: statementImportBlocked(summary, false, targetWasInspected),
    /** The account check or an unconfirmed date order still holds the import back. */
    blocked:
      statementImportBlocked(summary, accountConfirmed, targetWasInspected) || !dateOrderConfirmed,
    /** The order the dates are read in when they read both ways (QIF); null otherwise. */
    dateOrder,
    dateOrderConfirmed,
    pickDateOrder: setPickedDateOrder,
    /** Source path → the category chosen for it, or null (left to the rules). */
    sourceCategories,
    pickSourceCategory,
    options,
    open,
    switchAccount,
    changeStatement,
    reset,
  };
}
