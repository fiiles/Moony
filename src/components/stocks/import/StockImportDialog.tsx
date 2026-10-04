import { useCallback, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
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
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { useFileDrop } from '@/components/bank-accounts/csv-import/use-file-drop';
import {
  SUPPORTED_EXTENSIONS,
  fileNameFromPath,
} from '@/components/bank-accounts/csv-import/import-config';
import { useHistoryRecalculation } from '@/hooks/use-history-recalculation';
import { useStockImportMutations } from '@/hooks/use-stock-import-mutations';
import { useLanguage } from '@/i18n/I18nProvider';
import { stockImportApi } from '@/lib/tauri-api';
import type {
  CsvDateRange,
  SavedStockImportFormat,
  StockCsvInspection,
  StockImportInstrument,
  StockImportResult,
  StockImportUndoResult,
  StockInstrumentOverride,
  StockInstrumentResolution,
} from '@shared/schema';
import type { InstrumentEdit } from './InstrumentEditor';
import type { RememberFormat } from './MappingForm';
import { MappingStep } from './MappingStep';
import { ResultStep } from './ResultStep';
import { ReviewStep } from './ReviewStep';
import { SourceStep } from './SourceStep';
import {
  EMPTY_MAPPING,
  autoOverrideFor,
  buildStockImportConfig,
  fileStem,
  formatConfig,
  instrumentQuery,
  isMappingComplete,
  isSavedFormatSource,
  mappingFromInspection,
  mergeAutoOverrides,
  missingSymbolCount,
  normalizeTicker,
  rowStatusByLine,
  summarizePreview,
  updateOverride,
  type MappingState,
  type RowStatusByLine,
} from './import-config';
import { importErrorText } from './row-messages';
import { useInstrumentResolution } from './use-instrument-resolution';
import { useStockImportBatches, useStockImportFormats } from './use-stock-import-data';
import { useStockImportPreview } from './use-stock-import-preview';

type Step = 'file' | 'mapping' | 'review' | 'done';
const STEP_IDS: Step[] = ['file', 'mapping', 'review', 'done'];

/** Where the wizard starts: the first source of the list. */
const DEFAULT_SOURCE = 'xtb';

/** Dev bridge only: a path in localStorage replaces the native file dialog (shared with the bank wizard) */
const DEV_PATH_KEY = 'moony-dev-csv-path';

/** An import waiting for the user's confirmation to be undone. */
interface PendingUndo {
  id: string;
  fileName: string;
  count: number;
}

interface StockImportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * Stock import wizard (spec 2026-10-04-stock-csv-import-wizard-design.md §3):
 * file and source, columns, a review with duplicates, holdings checks and the
 * instruments confirmed on Yahoo Finance, then the result with the next steps
 * and undo. Rust reads and checks the file; this holds a path and a config.
 */
export function StockImportDialog({ open, onOpenChange }: StockImportDialogProps) {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const { getLocale } = useLanguage();
  const [, setLocation] = useLocation();
  const mutations = useStockImportMutations();
  const recalculating = useHistoryRecalculation().length > 0;

  const [step, setStep] = useState<Step>('file');
  const [filePath, setFilePath] = useState('');
  const [inspection, setInspection] = useState<StockCsvInspection | null>(null);
  const [mapping, setMapping] = useState<MappingState>(EMPTY_MAPPING);
  const [skipRows, setSkipRows] = useState(0);
  // The entry picked in the source list on step 1, and whether the user picked it
  // (an untouched default must not argue with what the file turns out to be).
  const [chosenSource, setChosenSource] = useState(DEFAULT_SOURCE);
  const [sourceTouched, setSourceTouched] = useState(false);
  // The full mapping form instead of the summary of a known source.
  const [editing, setEditing] = useState(false);
  const [remember, setRemember] = useState<RememberFormat>({ enabled: false, name: '' });
  // Duplicate rows (by file line) the user wants imported despite the match.
  const [importAnywayLines, setImportAnywayLines] = useState<number[]>([]);
  // The user's (and the lookups') choices per instrument.
  const [overrides, setOverrides] = useState<StockInstrumentOverride[]>([]);
  // Counts finished file reads, so layout fields restart from the new values.
  const [inspectRevision, setInspectRevision] = useState(0);
  const [isInspecting, setIsInspecting] = useState(false);
  const [isImporting, setIsImporting] = useState(false);
  const [result, setResult] = useState<StockImportResult | null>(null);
  const [importedRange, setImportedRange] = useState<CsvDateRange | null>(null);
  // What the review said about each row, kept for the result (its query is cleared by the import).
  const [reviewedRows, setReviewedRows] = useState<RowStatusByLine>(new Map());
  const [undone, setUndone] = useState<StockImportUndoResult | null>(null);
  const [pendingUndo, setPendingUndo] = useState<PendingUndo | null>(null);
  const [pendingFormatDelete, setPendingFormatDelete] = useState<SavedStockImportFormat | null>(
    null
  );
  // Bumped per file read; a response that is no longer the latest is dropped.
  const inspectRequest = useRef(0);

  const { data: formats = [] } = useStockImportFormats(open);
  const { data: batches = [] } = useStockImportBatches(open && step === 'file');

  // What an import would do with the current mapping; null until it is complete.
  const config = useMemo(
    () =>
      inspection && isMappingComplete(mapping)
        ? buildStockImportConfig(mapping, inspection, {
            skipRows,
            importAnywayLines,
            instrumentOverrides: overrides,
          })
        : null,
    [inspection, mapping, skipRows, importAnywayLines, overrides]
  );
  // The review runs from step 2 on, so step 3 opens with the table already there.
  const {
    preview,
    error: previewError,
    isUpdating,
    isLoadingFirst,
  } = useStockImportPreview({
    filePath,
    config,
    enabled: open && (step === 'mapping' || step === 'review'),
  });
  const summary = preview ? summarizePreview(preview) : null;

  // What Yahoo Finance knows about the instruments is applied as soon as it arrives.
  const handleResolved = useCallback(
    (asked: readonly StockImportInstrument[], answers: readonly StockInstrumentResolution[]) => {
      const byKey = new Map(answers.map((answer) => [answer.key, answer]));
      const automatic = asked.flatMap((instrument) => {
        const answer = byKey.get(instrument.key);
        const override = answer ? autoOverrideFor(instrument, answer) : null;
        return override ? [override] : [];
      });
      if (automatic.length > 0) setOverrides((current) => mergeAutoOverrides(current, automatic));
    },
    []
  );
  const resolution = useInstrumentResolution({
    enabled: open && step === 'review',
    instruments: preview?.instruments,
    onResolved: handleResolved,
  });

  const errorContext = { t, tc, locale: getLocale() };

  /**
   * Reads the file and (re)starts step 2 from what was found. `options` are the
   * manual overrides the backend accepts: the header line and the data rows to
   * skip. Reading the same file again with another layout keeps the form the
   * user has open and what they decided about remembering it.
   */
  const inspectFile = async (
    path: string,
    options: { headerRow?: number; skipRows?: number } = {},
    sameFile = false
  ) => {
    const requestId = ++inspectRequest.current;
    setIsInspecting(true);
    try {
      const inspected = await stockImportApi.inspect(path, {
        headerRow: options.headerRow ?? null,
        skipRows: options.skipRows ?? null,
      });
      if (requestId !== inspectRequest.current) return;

      setFilePath(path);
      setInspection(inspected);
      setMapping(mappingFromInspection(inspected));
      // A remembered format brings the rows to skip it was saved with.
      setSkipRows(options.skipRows ?? inspected.config?.skipRows ?? 0);
      if (!sameFile) {
        setEditing(false);
        // A file nothing recognised is a new format: remembering it is the default.
        setRemember({
          enabled: inspected.detectedSource == null,
          name: fileStem(fileNameFromPath(path)),
        });
      }
      setImportAnywayLines([]);
      setOverrides([]);
      resolution.reset();
      setStep('mapping');
    } catch (e: unknown) {
      if (requestId !== inspectRequest.current) return;
      toast.error(t('importWizard.toast.inspectFailed'), {
        description: importErrorText(e, errorContext),
      });
    } finally {
      if (requestId === inspectRequest.current) {
        setIsInspecting(false);
        setInspectRevision((revision) => revision + 1);
      }
    }
  };

  const handleFilePicked = (path: string) => {
    void inspectFile(path);
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
        filters: [{ name: 'CSV', extensions: [...SUPPORTED_EXTENSIONS] }],
      });
      if (selected) handleFilePicked(selected);
    } catch (e) {
      console.error('Error selecting file:', e);
    }
  };

  const isDragging = useFileDrop({
    enabled: open && step === 'file' && !isInspecting,
    onFile: handleFilePicked,
    onRejected: () => toast.error(t('importWizard.unsupportedFile')),
  });

  const handleLayoutChange = (layout: { headerRow: number; skipRows: number }) => {
    void inspectFile(filePath, layout, true);
  };

  const handleResetMapping = () => {
    if (!inspection) return;
    setMapping(mappingFromInspection(inspection));
    setRemember((current) => ({ ...current, enabled: false }));
    setEditing(false);
  };

  const toggleImportAnyway = (line: number, checked: boolean) => {
    setImportAnywayLines((lines) =>
      checked ? (lines.includes(line) ? lines : [...lines, line]) : lines.filter((l) => l !== line)
    );
  };

  const toggleInstrumentSkip = (instrument: StockImportInstrument, skip: boolean) => {
    setOverrides((current) => updateOverride(current, instrument.key, { skip }));
  };

  const editInstrument = (instrument: StockImportInstrument, edit: InstrumentEdit) => {
    const ticker = normalizeTicker(edit.ticker);
    const sameAsFile = ticker === (instrument.symbol ?? '').toUpperCase();
    const name = edit.name.trim();
    setOverrides((current) =>
      updateOverride(current, instrument.key, {
        // The file's own symbol needs no override; it is what the trades fall back to.
        ticker: sameAsFile ? null : ticker,
        name: name || null,
        ...(edit.currency ? { currency: edit.currency.toUpperCase() } : {}),
      })
    );
    if (edit.picked) {
      resolution.remember({
        key: instrument.key,
        candidates: [edit.picked],
        best: edit.picked,
        lookupFailed: false,
      });
    } else if (ticker !== (instrument.ticker ?? '').toUpperCase()) {
      void resolution.verify({ ...instrumentQuery(instrument), symbol: ticker });
    }
  };

  const resetWizard = () => {
    inspectRequest.current++;
    setStep('file');
    setFilePath('');
    setInspection(null);
    setMapping(EMPTY_MAPPING);
    setSkipRows(0);
    setEditing(false);
    setRemember({ enabled: false, name: '' });
    setImportAnywayLines([]);
    setOverrides([]);
    resolution.reset();
    setIsInspecting(false);
    setIsImporting(false);
    setResult(null);
    setImportedRange(null);
    setReviewedRows(new Map());
    setUndone(null);
  };

  const handleBack = () => {
    setStep(step === 'review' ? 'mapping' : 'file');
  };

  const handleClose = () => {
    // A write is in flight: closing now would hide the outcome.
    if (isImporting) return;
    resetWizard();
    setChosenSource(DEFAULT_SOURCE);
    setSourceTouched(false);
    onOpenChange(false);
  };

  const handleImport = async () => {
    if (!config || !preview || !inspection) return;
    setIsImporting(true);
    try {
      const imported = await mutations.importCsv.mutateAsync({ filePath, config });
      setResult(imported);
      setImportedRange(preview.dateRange);
      setReviewedRows(rowStatusByLine(preview.rows));
      setUndone(null);
      setStep('done');

      // The mapping is remembered once it has proved itself, and only if asked for.
      const name = remember.name.trim();
      if (remember.enabled && name && !isSavedFormatSource(mapping.source)) {
        try {
          const saved = await mutations.saveFormat.mutateAsync({
            name,
            headers: inspection.headers,
            config: formatConfig(config),
          });
          toast(t('importWizard.toast.formatSaved', { name: saved.name }));
        } catch (e: unknown) {
          toast.error(t('importWizard.toast.formatSaveFailed'), {
            description: importErrorText(e, errorContext),
          });
        }
      }
    } catch (e: unknown) {
      toast.error(t('importWizard.toast.importFailed'), {
        description: importErrorText(e, errorContext),
      });
    } finally {
      setIsImporting(false);
    }
  };

  const confirmUndo = async () => {
    if (!pendingUndo) return;
    try {
      const outcome = await mutations.undoBatch.mutateAsync(pendingUndo.id);
      toast(t('importWizard.toast.undone', { count: outcome.removed }));
      if (result?.batchId === pendingUndo.id) setUndone(outcome);
      setPendingUndo(null);
    } catch (e: unknown) {
      toast.error(t('importWizard.toast.undoFailed'), {
        description: importErrorText(e, errorContext),
      });
    }
  };

  const confirmFormatDelete = async () => {
    if (!pendingFormatDelete) return;
    try {
      await mutations.deleteFormat.mutateAsync(pendingFormatDelete.id);
      toast(t('importWizard.toast.formatDeleted'));
      if (chosenSource === pendingFormatDelete.id) setChosenSource(DEFAULT_SOURCE);
      setPendingFormatDelete(null);
    } catch (e: unknown) {
      toast.error(t('importWizard.toast.formatDeleteFailed'), {
        description: importErrorText(e, errorContext),
      });
    }
  };

  const handleShowStocks = () => {
    handleClose();
    setLocation('/stocks');
  };

  const handleOpenAnalysis = () => {
    handleClose();
    setLocation('/reports/stocks-analysis');
  };

  const rememberVisible = !isSavedFormatSource(mapping.source);
  const nameMissing = rememberVisible && remember.enabled && remember.name.trim() === '';
  const canReview = config != null && !isInspecting && !nameMissing;
  const canImport =
    config != null &&
    !isInspecting &&
    !isUpdating &&
    !previewError &&
    summary != null &&
    summary.willImport > 0 &&
    preview != null &&
    missingSymbolCount(preview) === 0 &&
    !resolution.isResolving;

  const steps = STEP_IDS.map((id) => ({ id, label: t(`importWizard.steps.${id}`) }));

  return (
    <>
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
            <DialogTitle>{t('importWizard.title')}</DialogTitle>
            <DialogDescription>{t('importWizard.lead')}</DialogDescription>
            <WizardSteps className="mt-4" steps={steps} current={step} />
          </DialogHeader>

          <div className="min-h-0 flex-1 overflow-y-auto px-px py-1">
            {step === 'file' && (
              <SourceStep
                source={chosenSource}
                onSourceChange={(source) => {
                  setChosenSource(source);
                  setSourceTouched(true);
                }}
                formats={formats}
                onDeleteFormat={setPendingFormatDelete}
                batches={batches}
                onUndoBatch={(batch) =>
                  setPendingUndo({
                    id: batch.id,
                    fileName: batch.fileName,
                    count: batch.remainingCount,
                  })
                }
                onChooseFile={() => void handleChooseFile()}
                isLoading={isInspecting}
                isDragging={isDragging}
              />
            )}

            {step === 'mapping' && inspection && (
              <MappingStep
                filePath={filePath}
                inspection={inspection}
                mapping={mapping}
                onMappingChange={(patch) => setMapping((current) => ({ ...current, ...patch }))}
                skipRows={skipRows}
                onLayoutChange={handleLayoutChange}
                inspectRevision={inspectRevision}
                isInspecting={isInspecting}
                chosenSource={sourceTouched ? chosenSource : null}
                formats={formats}
                editing={editing}
                onEditingChange={setEditing}
                onResetMapping={handleResetMapping}
                remember={rememberVisible ? remember : null}
                onRememberChange={(patch) => setRemember((current) => ({ ...current, ...patch }))}
              />
            )}

            {step === 'review' && (
              <ReviewStep
                ready={config != null}
                preview={preview}
                error={previewError}
                isUpdating={isUpdating}
                isLoadingFirst={isLoadingFirst}
                importAnywayLines={importAnywayLines}
                onToggleImportAnyway={toggleImportAnyway}
                overrides={overrides}
                resolutions={resolution.resolutions}
                progress={resolution.progress}
                isResolving={resolution.isResolving}
                onSkipVerification={resolution.skip}
                onRetryVerification={resolution.retry}
                onToggleSkip={toggleInstrumentSkip}
                onEditInstrument={editInstrument}
              />
            )}

            {step === 'done' && result && (
              <ResultStep
                result={result}
                fileName={fileNameFromPath(filePath)}
                dateRange={importedRange}
                reviewed={reviewedRows}
                recalculating={recalculating}
                undone={undone}
                isUndoing={mutations.undoBatch.isPending}
                onShowStocks={handleShowStocks}
                onOpenAnalysis={handleOpenAnalysis}
                onImportAnother={resetWizard}
                onUndo={() => {
                  if (!result.batchId) return;
                  setPendingUndo({
                    id: result.batchId,
                    fileName: fileNameFromPath(filePath),
                    count: result.imported,
                  });
                }}
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
                  onClick={() => setStep('review')}
                  disabled={!canReview}
                  loading={isInspecting}
                >
                  {t('importWizard.next')}
                </Button>
              </>
            )}
            {step === 'review' && (
              <>
                <Button variant="ghost" onClick={handleBack} disabled={isImporting}>
                  {tc('buttons.back')}
                </Button>
                <Button
                  onClick={() => void handleImport()}
                  disabled={!canImport || isImporting}
                  loading={isImporting || isUpdating}
                >
                  {summary
                    ? t('importWizard.importButton', { count: summary.willImport })
                    : t('importWizard.importButtonIdle')}
                </Button>
              </>
            )}
            {step === 'done' && <Button onClick={handleClose}>{tc('buttons.close')}</Button>}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDeleteDialog
        open={pendingUndo != null}
        onOpenChange={(next) => {
          if (!next) setPendingUndo(null);
        }}
        title={t('importWizard.undo.title')}
        description={`${t('importWizard.undo.description', {
          count: pendingUndo?.count ?? 0,
          file: pendingUndo?.fileName ?? '',
        })} ${t('importWizard.undo.consequences')}`}
        confirmLabel={t('importWizard.undo.confirm')}
        isPending={mutations.undoBatch.isPending}
        onConfirm={() => void confirmUndo()}
      />
      <ConfirmDeleteDialog
        open={pendingFormatDelete != null}
        onOpenChange={(next) => {
          if (!next) setPendingFormatDelete(null);
        }}
        title={t('importWizard.sources.saved.deleteTitle')}
        description={t('importWizard.sources.saved.deleteDescription', {
          name: pendingFormatDelete?.name ?? '',
        })}
        confirmLabel={t('importWizard.sources.saved.delete')}
        isPending={mutations.deleteFormat.isPending}
        onConfirm={() => void confirmFormatDelete()}
      />
    </>
  );
}
