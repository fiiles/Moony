import { useMemo, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { open as openFileDialog } from '@tauri-apps/plugin-dialog';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { WizardSteps } from '@/components/common/WizardSteps';
import { useAuth } from '@/hooks/use-auth';
import { useBankAccount } from '@/hooks/use-bank-accounts';
import { useCategories } from '@/hooks/use-categories';
import { useLanguage } from '@/i18n/I18nProvider';
import { bankAccountsApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import type {
  CsvCategoryOverride,
  CsvImportResult,
  CsvPreviewResult,
  StatementImportOptionsInput,
  StatementInspection,
} from '@shared/schema';
import { FileSelectStep } from './csv-import/FileSelectStep';
import { ImportResultStep } from './csv-import/ImportResultStep';
import { MappingStep } from './csv-import/MappingStep';
import { PreviewStep } from './csv-import/PreviewStep';
import { StatementSummaryCard } from './csv-import/StatementSummaryCard';
import {
  AUTO_PRESET,
  AUTO_VALUE,
  EMPTY_MAPPING,
  SUPPORTED_EXTENSIONS,
  buildImportConfig,
  encodingArgument,
  fileNameFromPath,
  importSteps,
  isMappingComplete,
  isSupportedImportPath,
  mappingFromInspection,
  presetIdForInstitution,
  statementImportBlocked,
  summarizePreview,
  type ImportKind,
  type ImportStep,
  type MappingState,
} from './csv-import/import-config';
import {
  useCsvImportMutation,
  useStatementImportMutation,
} from './csv-import/use-csv-import-mutations';
import { useCsvPreview } from './csv-import/use-csv-preview';
import { useFileDrop } from './csv-import/use-file-drop';
import { useStatementPreview } from './csv-import/use-statement-preview';

interface StatementImportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  accountId: string;
  /** Pre-selects the bank preset seeded for this institution. */
  institutionId?: string | null;
  /**
   * Show what was imported (the page sets its filter to `result.dateRange`).
   * Called as soon as an import finishes, so the list behind the dialog is
   * already right even if it is simply closed, and again by "Show imported
   * transactions". Not called when the import went to another account (D9).
   */
  onImported?: (result: CsvImportResult) => void;
  /** Filter the page to uncategorized transactions ("Review N uncategorized"). */
  onReviewUncategorized?: () => void;
  /** Run the page's auto-categorization (after the suggested rule pack was enabled). */
  onAutoCategorize?: () => void | Promise<void>;
}

/** Dev bridge only: a path in localStorage replaces the native file dialog */
const DEV_PATH_KEY = 'moony-dev-csv-path';

/**
 * Statement import wizard (design system prototype csv-import.html): file and
 * bank, columns, the preview with duplicates and the category every row would
 * get (fixable inline before the write), then the result with the next steps.
 * A statement file (CAMT.053, MT940, OFX, QIF) names its fields itself: it skips
 * the columns and its preview opens with the statement summary (account check,
 * balances, the statement picker).
 */
export function StatementImportDialog({
  open,
  onOpenChange,
  accountId,
  institutionId,
  onImported,
  onReviewUncategorized,
  onAutoCategorize,
}: StatementImportDialogProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const { getLocale } = useLanguage();
  const { user } = useAuth();
  // Inspect, preview and import decode a legacy-encoded file the same way.
  const country = user?.country ?? undefined;
  // The account the import writes to: the page's, until the user switches to the
  // account the statement belongs to (D9). Derived, so a new page account applies at once.
  const [switchedAccountId, setTargetAccountId] = useState<string | null>(null);
  const targetAccountId = switchedAccountId ?? accountId;
  const { account } = useBankAccount(targetAccountId || undefined);
  const { categories } = useCategories();

  const [step, setStep] = useState<ImportStep>('file');
  // What the open file is; null until one was read.
  const [kind, setKind] = useState<ImportKind | null>(null);
  const [statementInspection, setStatementInspection] = useState<StatementInspection | null>(null);
  // The account `statementInspection` was read for: its account check is relative to it.
  const [inspectedAccountId, setInspectedAccountId] = useState<string | null>(null);
  const [statementIndex, setStatementIndex] = useState(0);
  // "Import into this account anyway" for a statement of another account (D9).
  const [accountConfirmed, setAccountConfirmed] = useState(false);
  const [filePath, setFilePath] = useState('');
  const [inspection, setInspection] = useState<CsvPreviewResult | null>(null);
  const [mapping, setMapping] = useState<MappingState>(EMPTY_MAPPING);
  const [skipRows, setSkipRows] = useState(0);
  // Duplicate rows (by file line) the user wants imported despite the match.
  const [importAnywayLines, setImportAnywayLines] = useState<number[]>([]);
  // Categories picked in the preview, by file line.
  const [overrides, setOverrides] = useState<CsvCategoryOverride[]>([]);
  // Counts finished file reads, so layout fields restart from the new values.
  const [inspectRevision, setInspectRevision] = useState(0);
  // The user's (or the detection's) preset; null = nothing chosen yet.
  const [presetOverride, setPresetOverride] = useState<string | null>(null);
  const [presetDetected, setPresetDetected] = useState(false);
  // `AUTO_VALUE` or the encoding the user picked for the file that is open.
  const [encodingChoice, setEncodingChoice] = useState<string>(AUTO_VALUE);
  const [isInspecting, setIsInspecting] = useState(false);
  const [isImporting, setIsImporting] = useState(false);
  const [result, setResult] = useState<CsvImportResult | null>(null);
  // The account `result` was written to, fixed when the write started.
  const [importedAccountId, setImportedAccountId] = useState<string | null>(null);
  const { account: importedAccount } = useBankAccount(importedAccountId ?? undefined);
  const [previewPage, setPreviewPage] = useState(0);
  const [dateConfirmed, setDateConfirmed] = useState(false);
  const [manualCount, setManualCount] = useState(0);
  // Bumped per file read; a response that is no longer the latest is dropped.
  const inspectRequest = useRef(0);

  const importMutation = useCsvImportMutation(targetAccountId);
  const statementMutation = useStatementImportMutation();

  const { data: presets = [] } = useQuery({
    queryKey: ['csv-presets'],
    queryFn: () => bankAccountsApi.getCsvPresets(),
    enabled: open,
  });

  const institutionPreset = useMemo(
    () => presetIdForInstitution(presets, institutionId),
    [presets, institutionId]
  );
  const presetChoice = presetOverride ?? institutionPreset ?? AUTO_PRESET;
  /**
   * The preset to force when reading a file; undefined = detect it. A preset
   * that was only detected (not picked) is never forced onto the next read:
   * another file may be from another bank.
   */
  const explicitPresetId =
    presetDetected || presetChoice === AUTO_PRESET ? undefined : presetChoice;

  /**
   * Reads the file and (re)starts step 2 from what was found: columns, formats,
   * preset. `options` are the manual overrides the backend accepts.
   */
  const inspectFile = async (
    path: string,
    options: { presetId?: string; headerRow?: number; skipRows?: number; encoding?: string } = {}
  ) => {
    const requestId = ++inspectRequest.current;
    setIsInspecting(true);
    try {
      const parsed = await bankAccountsApi.parseCsvFile(
        path,
        undefined, // always detect the delimiter; bank formats change
        options.skipRows,
        options.headerRow,
        options.presetId,
        options.encoding,
        country
      );
      if (requestId !== inspectRequest.current) return;

      setFilePath(path);
      setInspection(parsed);
      const detectedMapping = mappingFromInspection(parsed);
      setMapping(detectedMapping);
      setPreviewPage(0);
      setDateConfirmed(!parsed.dateFormatAmbiguous);
      setSkipRows(options.skipRows ?? 0);
      setImportAnywayLines([]);
      setOverrides([]);
      if (options.presetId === undefined) {
        // Auto-detect: show what was found.
        setPresetOverride(parsed.detectedPresetId ?? AUTO_PRESET);
        setPresetDetected(parsed.detectedPresetId != null);
      } else {
        setPresetOverride(options.presetId);
        setPresetDetected(false);
      }
      setStep(
        parsed.detectedPresetId && !parsed.dateFormatAmbiguous && isMappingComplete(detectedMapping)
          ? 'preview'
          : 'mapping'
      );
    } catch (e: unknown) {
      if (requestId !== inspectRequest.current) return;
      toast.error(tc('status.error'), { description: translateApiError(e as Error, tc) });
    } finally {
      if (requestId === inspectRequest.current) {
        setIsInspecting(false);
        setInspectRevision((revision) => revision + 1);
      }
    }
  };

  /**
   * Reads the file as a statement first. Not a statement (null): the CSV path,
   * unchanged. A statement goes straight to its preview.
   */
  const openFile = async (path: string) => {
    const requestId = ++inspectRequest.current;
    setIsInspecting(true);
    try {
      const statement = await bankAccountsApi.inspectStatementFile(targetAccountId, path, country);
      if (requestId !== inspectRequest.current) return;
      if (statement == null) {
        setKind('csv');
        setStatementInspection(null);
        void inspectFile(path, { presetId: explicitPresetId });
        return;
      }
      setKind('statement');
      setFilePath(path);
      // A CSV read before must not preview this file with its columns.
      setInspection(null);
      setStatementInspection(statement);
      setInspectedAccountId(targetAccountId);
      setStatementIndex(statement.defaultStatementIndex);
      setImportAnywayLines([]);
      setOverrides([]);
      setAccountConfirmed(false);
      setPreviewPage(0);
      setStep('preview');
    } catch (e: unknown) {
      if (requestId !== inspectRequest.current) return;
      toast.error(tc('status.error'), { description: translateApiError(e as Error, tc) });
    } finally {
      if (requestId === inspectRequest.current) setIsInspecting(false);
    }
  };

  const handleFilePicked = (path: string) => {
    // The last choice was made for another file.
    setEncodingChoice(AUTO_VALUE);
    void openFile(path);
  };

  /**
   * "Import into {account}" (D9): the statement's own account becomes the target
   * and the file is read again for it, so the account check reads `selected`.
   */
  const handleSwitchAccount = async (matchedAccountId: string) => {
    const previousTarget = switchedAccountId;
    setTargetAccountId(matchedAccountId === accountId ? null : matchedAccountId);
    setAccountConfirmed(false);
    // Duplicates are another account's now; the categories picked still apply.
    setImportAnywayLines([]);
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
      setStatementInspection(statement);
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

  const handleStatementChange = (index: number) => {
    setStatementIndex(index);
    // Entry ordinals belong to the statement they were picked in.
    setImportAnywayLines([]);
    setOverrides([]);
    setAccountConfirmed(false);
    setPreviewPage(0);
  };

  const handleChooseFile = async () => {
    if (import.meta.env.DEV) {
      try {
        const devPath = localStorage.getItem(DEV_PATH_KEY);
        if (devPath) {
          handleFilePicked(devPath);
          return;
        }
      } catch {
        /* private mode */
      }
    }
    try {
      const selected = await openFileDialog({
        multiple: false,
        filters: [{ name: t('statementImport.fileFilter'), extensions: [...SUPPORTED_EXTENSIONS] }],
      });
      if (selected) handleFilePicked(selected);
    } catch (e) {
      console.error('Error selecting file:', e);
    }
  };

  const isDragging = useFileDrop({
    enabled: open && step === 'file' && !isInspecting,
    accepts: isSupportedImportPath,
    onFile: handleFilePicked,
    onRejected: () =>
      toast.error(t('csvImport.unsupportedFile'), {
        description: t('statementImport.supportedFormats'),
      }),
  });

  const handlePresetChange = (value: string) => {
    setPresetOverride(value);
    setPresetDetected(false);
    // Once a file is open, another preset means reading it again with that preset.
    if (step !== 'file' && filePath) {
      void inspectFile(filePath, {
        presetId: value === AUTO_PRESET ? undefined : value,
        encoding: encodingArgument(encodingChoice),
      });
    }
  };

  const handleLayoutChange = (layout: { headerRow: number; skipRows: number }) => {
    void inspectFile(filePath, {
      presetId: explicitPresetId,
      encoding: encodingArgument(encodingChoice),
      ...layout,
    });
  };

  const handleEncodingChange = (value: string) => {
    setEncodingChoice(value);
    if (filePath) {
      void inspectFile(filePath, { presetId: explicitPresetId, encoding: encodingArgument(value) });
    }
  };

  const toggleImportAnyway = (line: number, checked: boolean) => {
    setImportAnywayLines((lines) =>
      checked ? (lines.includes(line) ? lines : [...lines, line]) : lines.filter((l) => l !== line)
    );
  };

  const setOverride = (line: number, categoryId: string | null) => {
    setOverrides((current) => {
      const rest = current.filter((o) => o.line !== line);
      return categoryId ? [...rest, { line, categoryId }] : rest;
    });
  };

  // What an import would do with the current mapping; null until it is complete.
  const config = useMemo(
    () =>
      inspection && dateConfirmed && isMappingComplete(mapping)
        ? buildImportConfig(mapping, inspection, skipRows, importAnywayLines, overrides)
        : null,
    [inspection, mapping, skipRows, importAnywayLines, overrides, dateConfirmed]
  );
  const previewConfig = useMemo(
    () => (config ? { ...config, previewOffset: previewPage * 100, previewLimit: 100 } : null),
    [config, previewPage]
  );
  // The preview runs from step 2 on, so step 3 opens with the table already there.
  const csvPreview = useCsvPreview({
    accountId: targetAccountId,
    filePath,
    config: kind === 'csv' ? previewConfig : null,
    enabled: open && kind === 'csv' && (step === 'mapping' || step === 'preview'),
  });

  // What a statement import would do; null while no statement is open.
  const statementOptions = useMemo<StatementImportOptionsInput | null>(
    () =>
      kind === 'statement' && statementInspection
        ? {
            statementIndex,
            importAnywayLines: [...importAnywayLines].sort((a, b) => a - b),
            categoryOverrides: [...overrides].sort((a, b) => a.line - b.line),
          }
        : null,
    [kind, statementInspection, statementIndex, importAnywayLines, overrides]
  );
  const statementPreviewOptions = useMemo(
    () =>
      statementOptions
        ? { ...statementOptions, previewOffset: previewPage * 100, previewLimit: 100 }
        : null,
    [statementOptions, previewPage]
  );
  const statementPreview = useStatementPreview({
    accountId: targetAccountId,
    filePath,
    options: statementPreviewOptions,
    country,
    enabled: open && kind === 'statement' && step === 'preview',
  });
  const statementSummary = statementInspection?.statements.find(
    (statement) => statement.index === statementIndex
  );

  const {
    preview,
    error: previewError,
    isUpdating,
    isLoadingFirst,
  } = kind === 'statement' ? statementPreview : csvPreview;
  const summary = preview ? summarizePreview(preview, importAnywayLines) : null;

  const reset = () => {
    inspectRequest.current++;
    setStep('file');
    setKind(null);
    setStatementInspection(null);
    setInspectedAccountId(null);
    setStatementIndex(0);
    setAccountConfirmed(false);
    setTargetAccountId(null);
    setFilePath('');
    setInspection(null);
    setMapping(EMPTY_MAPPING);
    setSkipRows(0);
    setImportAnywayLines([]);
    setOverrides([]);
    setPresetOverride(null);
    setPresetDetected(false);
    setEncodingChoice(AUTO_VALUE);
    setIsInspecting(false);
    setIsImporting(false);
    setResult(null);
    setImportedAccountId(null);
    setManualCount(0);
  };

  const handleBack = () => {
    // A statement has no columns to edit: back means another file, which starts over
    // as a file of unknown kind for the page's account (the switch was for this one).
    if (step === 'preview' && kind === 'statement') {
      inspectRequest.current++; // a switch's read still in flight no longer applies
      setIsInspecting(false);
      setKind(null);
      setStatementInspection(null);
      setInspectedAccountId(null);
      setAccountConfirmed(false);
      setTargetAccountId(null);
      setStep('file');
      return;
    }
    if (step === 'preview') {
      setStep('mapping');
      return;
    }
    // The next file may be from another bank: forget what was detected for this one.
    if (presetDetected) {
      setPresetOverride(null);
      setPresetDetected(false);
    }
    setStep('file');
  };

  const handleImport = async () => {
    if (!preview) return;
    // The account this write goes to; the result step and its next steps act on it.
    const writeAccountId = targetAccountId;
    // Preview and import get the same options, so what the user saw is what is written.
    const write =
      kind === 'statement'
        ? statementOptions &&
          (() =>
            statementMutation.mutateAsync({
              accountId: writeAccountId,
              filePath,
              options: statementOptions,
              country,
            }))
        : config && (() => importMutation.mutateAsync({ filePath, config }));
    if (!write) return;
    setIsImporting(true);
    try {
      const imported = await write();
      setManualCount(overrides.length);
      setResult(imported);
      setImportedAccountId(writeAccountId);
      setStep('done');
      // The page behind shows its own account; another account's rows are not there.
      if (writeAccountId === accountId) onImported?.(imported);
    } catch (e: unknown) {
      toast.error(tc('status.error'), { description: translateApiError(e as Error, tc) });
    } finally {
      setIsImporting(false);
    }
  };

  const handleClose = () => {
    // A write is in flight: closing now would hide the outcome.
    if (isImporting) return;
    reset();
    onOpenChange(false);
  };

  const handleShowImported = () => {
    if (result) onImported?.(result);
    handleClose();
  };

  const handleReviewUncategorized = () => {
    onReviewUncategorized?.();
    handleClose();
  };

  const canPreview = config != null && !isInspecting;
  // The account check (D9) is relative to the account the file was read for: after a
  // switch it holds for the new target once the file was read again for it (until then
  // `isInspecting` holds the import back).
  const targetWasInspected = inspectedAccountId === targetAccountId;
  const statementBlocked =
    kind === 'statement' &&
    statementImportBlocked(statementSummary, accountConfirmed, targetWasInspected);
  const canImport =
    (kind === 'statement' ? statementOptions != null && !statementBlocked : config != null) &&
    !isInspecting &&
    !isUpdating &&
    !previewError &&
    summary != null &&
    (summary.willImport > 0 || summary.willPromote > 0);

  const steps = importSteps(kind ?? 'csv').map((id) => ({
    id,
    label: t(`csvImport.steps.${id === 'mapping' ? 'columns' : id}`),
  }));

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) handleClose();
      }}
    >
      <DialogContent
        className="flex max-h-[90vh] max-w-[980px] flex-col"
        hideCloseButton={isImporting}
      >
        <DialogHeader>
          <DialogTitle>
            {account ? t('csvImport.titleFor', { account: account.name }) : t('csvImport.title')}
          </DialogTitle>
          <DialogDescription>{t('csvImport.lead')}</DialogDescription>
          <WizardSteps className="mt-4" steps={steps} current={step} />
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-px py-1">
          {step === 'file' && (
            <FileSelectStep
              accountCurrency={account?.currency}
              presets={presets}
              presetChoice={presetChoice}
              onPresetChange={handlePresetChange}
              onChooseFile={() => void handleChooseFile()}
              isLoading={isInspecting}
              isDragging={isDragging}
            />
          )}

          {step === 'mapping' && inspection && (
            <MappingStep
              key={inspectRevision}
              filePath={filePath}
              inspection={inspection}
              dateConfirmed={dateConfirmed}
              mapping={mapping}
              onMappingChange={(patch) => {
                setMapping((current) => ({ ...current, ...patch }));
                setPreviewPage(0);
                if (patch.dateFormat) setDateConfirmed(true);
              }}
              skipRows={skipRows}
              onLayoutChange={handleLayoutChange}
              inspectRevision={inspectRevision}
              presets={presets}
              presetChoice={presetChoice}
              presetDetected={presetDetected}
              onPresetChange={handlePresetChange}
              isInspecting={isInspecting}
              encodingChoice={encodingChoice}
              onEncodingChange={handleEncodingChange}
            />
          )}

          {step === 'preview' &&
            kind === 'statement' &&
            statementInspection &&
            statementSummary && (
              <StatementSummaryCard
                inspection={statementInspection}
                summary={statementSummary}
                onStatementChange={handleStatementChange}
                fileName={fileNameFromPath(filePath)}
                accountCurrency={account?.currency}
                confirmRequired={statementImportBlocked(
                  statementSummary,
                  false,
                  targetWasInspected
                )}
                accountConfirmed={accountConfirmed}
                onAccountConfirmedChange={setAccountConfirmed}
                onSwitchAccount={(id) => void handleSwitchAccount(id)}
                busy={isInspecting || isImporting}
                locale={getLocale()}
              />
            )}

          {step === 'preview' && (
            <PreviewStep
              kind={kind ?? 'csv'}
              page={previewPage}
              onPageChange={setPreviewPage}
              ready={kind === 'statement' ? statementOptions != null : config != null}
              preview={preview}
              error={previewError}
              isUpdating={isUpdating}
              isLoadingFirst={isLoadingFirst}
              importAnywayLines={importAnywayLines}
              onToggleImportAnyway={toggleImportAnyway}
              overrides={overrides}
              onOverride={setOverride}
              categories={categories}
              accountCurrency={account?.currency}
              locale={getLocale()}
            />
          )}

          {step === 'done' && result && importedAccountId != null && (
            <ImportResultStep
              kind={kind ?? 'csv'}
              accountId={accountId}
              importedAccountId={importedAccountId}
              result={result}
              account={importedAccount}
              locale={getLocale()}
              fileName={fileNameFromPath(filePath)}
              manualCount={manualCount}
              onShowImported={onImported ? handleShowImported : undefined}
              onReviewUncategorized={onReviewUncategorized ? handleReviewUncategorized : undefined}
              onAutoCategorize={onAutoCategorize}
              onClose={handleClose}
            />
          )}
        </div>

        <DialogFooter>
          {step === 'file' && (
            <Button variant="ghost" onClick={handleClose}>
              {tc('buttons.cancel')}
            </Button>
          )}
          {step === 'mapping' && (
            <>
              <Button variant="ghost" onClick={handleBack} disabled={isInspecting}>
                {tc('buttons.back')}
              </Button>
              <Button
                onClick={() => setStep('preview')}
                disabled={!canPreview}
                loading={isInspecting}
              >
                {t('csvImport.showPreview')}
              </Button>
            </>
          )}
          {step === 'preview' && (
            <>
              <Button variant="ghost" onClick={handleBack} disabled={isImporting}>
                {kind === 'statement' ? t('statementImport.back') : t('csvImport.editColumns')}
              </Button>
              <Button
                onClick={() => void handleImport()}
                disabled={!canImport || isImporting}
                loading={isImporting || isUpdating}
              >
                {summary
                  ? t('csvImport.importButton', { count: summary.willImport })
                  : t('csvImport.importButtonIdle')}
              </Button>
            </>
          )}
          {step === 'done' && <Button onClick={handleClose}>{tc('buttons.close')}</Button>}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
