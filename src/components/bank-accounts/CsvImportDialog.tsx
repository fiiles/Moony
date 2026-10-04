import { useMemo, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
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
import { useBankAccount } from '@/hooks/use-bank-accounts';
import { useCategories } from '@/hooks/use-categories';
import { useLanguage } from '@/i18n/I18nProvider';
import { bankAccountsApi, categorizationApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import type { CsvCategoryOverride, CsvImportResult, CsvPreviewResult } from '@shared/schema';
import { FileSelectStep } from './csv-import/FileSelectStep';
import { ImportResultStep } from './csv-import/ImportResultStep';
import { MappingStep } from './csv-import/MappingStep';
import { PreviewStep } from './csv-import/PreviewStep';
import {
  AUTO_PRESET,
  EMPTY_MAPPING,
  SUPPORTED_EXTENSIONS,
  buildImportConfig,
  fileNameFromPath,
  isMappingComplete,
  mappingFromInspection,
  presetIdForInstitution,
  summarizePreview,
  type MappingState,
} from './csv-import/import-config';
import { useCsvImportMutation } from './csv-import/use-csv-import-mutations';
import { useCsvPreview } from './csv-import/use-csv-preview';
import { useFileDrop } from './csv-import/use-file-drop';

interface CsvImportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  accountId: string;
  /** Pre-selects the bank preset seeded for this institution. */
  institutionId?: string | null;
  /**
   * Show what was imported (the page sets its filter to `result.dateRange`).
   * Called as soon as an import finishes, so the list behind the dialog is
   * already right even if it is simply closed, and again by "Show imported
   * transactions".
   */
  onImported?: (result: CsvImportResult) => void;
  /** Filter the page to uncategorized transactions ("Review N uncategorized"). */
  onReviewUncategorized?: () => void;
  /** Run the page's auto-categorization (after the suggested rule pack was enabled). */
  onAutoCategorize?: () => void | Promise<void>;
}

type Step = 'file' | 'mapping' | 'preview' | 'done';
const STEP_IDS: Step[] = ['file', 'mapping', 'preview', 'done'];

/** Dev bridge only: a path in localStorage replaces the native file dialog */
const DEV_PATH_KEY = 'moony-dev-csv-path';

/**
 * CSV import wizard (design system prototype csv-import.html): file and bank,
 * columns, the preview with duplicates and the category every row would get
 * (fixable inline before the write), then the result with the next steps.
 */
export function CsvImportDialog({
  open,
  onOpenChange,
  accountId,
  institutionId,
  onImported,
  onReviewUncategorized,
  onAutoCategorize,
}: CsvImportDialogProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const { getLocale } = useLanguage();
  const [, setLocation] = useLocation();
  const { account } = useBankAccount(accountId || undefined);
  const { categories } = useCategories();

  const [step, setStep] = useState<Step>('file');
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
  const [isInspecting, setIsInspecting] = useState(false);
  const [isImporting, setIsImporting] = useState(false);
  const [result, setResult] = useState<CsvImportResult | null>(null);
  const [manualCount, setManualCount] = useState(0);
  // Bumped per file read; a response that is no longer the latest is dropped.
  const inspectRequest = useRef(0);

  const importMutation = useCsvImportMutation(accountId);

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
    options: { presetId?: string; headerRow?: number; skipRows?: number } = {}
  ) => {
    const requestId = ++inspectRequest.current;
    setIsInspecting(true);
    try {
      const parsed = await bankAccountsApi.parseCsvFile(
        path,
        undefined, // always detect the delimiter; bank formats change
        options.skipRows,
        options.headerRow,
        options.presetId
      );
      if (requestId !== inspectRequest.current) return;

      setFilePath(path);
      setInspection(parsed);
      setMapping(mappingFromInspection(parsed));
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
      setStep('mapping');
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

  const handleFilePicked = (path: string) => {
    void inspectFile(path, { presetId: explicitPresetId });
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
    onRejected: () => toast.error(t('csvImport.unsupportedFile')),
  });

  const handlePresetChange = (value: string) => {
    setPresetOverride(value);
    setPresetDetected(false);
    // Once a file is open, another preset means reading it again with that preset.
    if (step !== 'file' && filePath) {
      void inspectFile(filePath, { presetId: value === AUTO_PRESET ? undefined : value });
    }
  };

  const handleLayoutChange = (layout: { headerRow: number; skipRows: number }) => {
    void inspectFile(filePath, { presetId: explicitPresetId, ...layout });
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
      inspection && isMappingComplete(mapping)
        ? buildImportConfig(mapping, inspection, skipRows, importAnywayLines, overrides)
        : null,
    [inspection, mapping, skipRows, importAnywayLines, overrides]
  );
  // The preview runs from step 2 on, so step 3 opens with the table already there.
  const {
    preview,
    error: previewError,
    isUpdating,
    isLoadingFirst,
  } = useCsvPreview({
    accountId,
    filePath,
    config,
    enabled: open && (step === 'mapping' || step === 'preview'),
  });
  const summary = preview ? summarizePreview(preview, importAnywayLines) : null;

  const reset = () => {
    inspectRequest.current++;
    setStep('file');
    setFilePath('');
    setInspection(null);
    setMapping(EMPTY_MAPPING);
    setSkipRows(0);
    setImportAnywayLines([]);
    setOverrides([]);
    setPresetOverride(null);
    setPresetDetected(false);
    setIsInspecting(false);
    setIsImporting(false);
    setResult(null);
    setManualCount(0);
  };

  const handleBack = () => {
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
    if (!config || !preview) return;
    setIsImporting(true);
    try {
      const imported = await importMutation.mutateAsync({ filePath, config });
      // A category picked by hand becomes a learned payee, like a manual change in the ledger.
      const byLine = new Map(preview.rows.map((row) => [row.line, row]));
      let learned = 0;
      for (const override of overrides) {
        const row = byLine.get(override.line);
        if (!row) continue;
        learned += 1;
        if (row.counterparty) {
          categorizationApi.learn(row.counterparty, null, override.categoryId).catch((e) => {
            console.error('Could not learn the picked category:', e);
          });
        }
      }
      setManualCount(learned);
      setResult(imported);
      setStep('done');
      onImported?.(imported);
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

  const openRules = () => {
    handleClose();
    setLocation('/categorization-rules');
  };

  const canPreview = config != null && !isInspecting;
  const canImport =
    config != null &&
    !isInspecting &&
    !isUpdating &&
    !previewError &&
    summary != null &&
    summary.willImport > 0;

  const steps = STEP_IDS.map((id) => ({
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
              filePath={filePath}
              inspection={inspection}
              mapping={mapping}
              onMappingChange={(patch) => setMapping((current) => ({ ...current, ...patch }))}
              skipRows={skipRows}
              onLayoutChange={handleLayoutChange}
              inspectRevision={inspectRevision}
              presets={presets}
              presetChoice={presetChoice}
              presetDetected={presetDetected}
              onPresetChange={handlePresetChange}
              isInspecting={isInspecting}
            />
          )}

          {step === 'preview' && (
            <PreviewStep
              ready={config != null}
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
              onOpenRules={openRules}
            />
          )}

          {step === 'done' && result && (
            <ImportResultStep
              accountId={accountId}
              result={result}
              account={account}
              locale={getLocale()}
              fileName={fileNameFromPath(filePath)}
              manualCount={manualCount}
              onShowImported={onImported ? handleShowImported : undefined}
              onReviewUncategorized={onReviewUncategorized ? handleReviewUncategorized : undefined}
              onAutoCategorize={onAutoCategorize}
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
                {tc('buttons.back')}
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
