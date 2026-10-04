import { useTranslation } from 'react-i18next';
import { save } from '@tauri-apps/plugin-dialog';
import { writeTextFile } from '@tauri-apps/plugin-fs';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { Upload } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { useLanguage } from '@/i18n/I18nProvider';
import { cn } from '@/lib/utils';
import type { SavedStockImportFormat, StockImportBatch } from '@shared/schema';
import { RecentImports } from './RecentImports';
import {
  SOURCE_IDS,
  buildSampleCsv,
  isSavedFormatSource,
  isSourceId,
  sourceHelpUrl,
  type SourceId,
} from './import-config';

const MOONY_COLUMNS = ['date', 'type', 'symbol', 'quantity', 'price', 'currency', 'name'] as const;

interface SourceStepProps {
  /** The selected entry: a source id or `format:<id>`. */
  source: string;
  onSourceChange: (source: string) => void;
  formats: readonly SavedStockImportFormat[];
  /** Ask to forget a remembered format. */
  onDeleteFormat: (format: SavedStockImportFormat) => void;
  batches: readonly StockImportBatch[];
  onUndoBatch: (batch: StockImportBatch) => void;
  /** Open the native file dialog. */
  onChooseFile: () => void;
  /** A file is being read. */
  isLoading: boolean;
  /** A file is being dragged over the window. */
  isDragging: boolean;
}

/**
 * Step 1 (spec §3): where the export comes from with the guide of the chosen
 * source on the left; the drop zone, the sample file and the recent imports on
 * the right.
 */
export function SourceStep({
  source,
  onSourceChange,
  formats,
  onDeleteFormat,
  batches,
  onUndoBatch,
  onChooseFile,
  isLoading,
  isDragging,
}: SourceStepProps) {
  const { t } = useTranslation('stocks');
  // The supported UI language (`cs` or `en`), whatever the detector found first.
  const { language } = useLanguage();

  const downloadSample = async () => {
    try {
      const path = await save({
        defaultPath: 'moony-sample-trades.csv',
        filters: [{ name: 'CSV', extensions: ['csv'] }],
      });
      if (!path) return;
      await writeTextFile(path, buildSampleCsv(language));
      toast(t('importWizard.file.sampleSaved'));
    } catch (error) {
      console.error('Could not save the sample CSV:', error);
      toast.error(t('importWizard.file.sampleFailed'), { description: String(error) });
    }
  };

  const openHelp = async (url: string) => {
    try {
      await openUrl(url);
    } catch (error) {
      console.error('Could not open the export help page:', error);
      toast.error(t('importWizard.file.helpFailed'), { description: String(error) });
    }
  };

  // The built-in sources, the remembered formats, and "Jiný broker" last (spec §3).
  const entry = (id: SourceId) => ({ id, label: t(`importWizard.sources.${id}.name`) });
  const entries: { id: string; label: string; saved?: boolean }[] = [
    ...SOURCE_IDS.filter((id) => id !== 'custom').map(entry),
    ...formats.map((format) => ({ id: format.id, label: format.name, saved: true })),
    entry('custom'),
  ];
  const selectedFormat = formats.find((format) => format.id === source);
  const helpUrl = sourceHelpUrl(source, language);
  const brokerName = isSourceId(source) ? t(`importWizard.sources.${source}.name`) : '';

  const row = (item: { id: string; label: string; saved?: boolean }) => {
    const selected = item.id === source;
    return (
      <label
        key={item.id}
        htmlFor={`stock-source-${item.id}`}
        className={cn(
          'flex cursor-pointer items-center gap-2.5 rounded-r2 border px-3 py-2 text-table font-600 transition-colors duration-fast',
          selected
            ? 'border-line-strong bg-well text-ink'
            : 'border-transparent text-ink-2 hover:bg-well'
        )}
      >
        <RadioGroupItem id={`stock-source-${item.id}`} value={item.id} disabled={isLoading} />
        <span className="min-w-0 flex-1 truncate">{item.label}</span>
        {item.saved && (
          <span className="shrink-0 text-micro font-500 text-ink-4">
            {t('importWizard.file.savedTag')}
          </span>
        )}
      </label>
    );
  };

  return (
    <div className="grid grid-cols-[1fr_1.15fr] gap-6">
      <div className="min-w-0">
        <h4 className="mb-2 mt-1 text-caption font-700 text-ink-2">
          {t('importWizard.file.sourceHeading')}
        </h4>
        <RadioGroup
          value={source}
          onValueChange={onSourceChange}
          className="gap-0.5"
          aria-label={t('importWizard.file.sourceHeading')}
        >
          {entries.map(row)}
        </RadioGroup>

        <div className="mt-4 rounded-r3 bg-well px-4 py-3.5">
          {isSourceId(source) ? (
            <SourceGuide source={source} />
          ) : (
            <>
              <h4 className="m-0 mb-1.5 text-caption font-700 text-ink-2">
                {t('importWizard.sources.saved.title')}
              </h4>
              <p className="m-0 text-table leading-[1.5] text-ink-2">
                {t('importWizard.sources.saved.guide')}
              </p>
            </>
          )}
          {helpUrl && (
            <button
              type="button"
              className="mt-2.5 text-caption font-600 text-ink-3 underline underline-offset-[3px] hover:text-ink"
              onClick={() => void openHelp(helpUrl)}
            >
              {t('importWizard.file.howToExport', { name: brokerName })} →
            </button>
          )}
          {selectedFormat && isSavedFormatSource(source) && (
            <Button
              type="button"
              variant="danger"
              size="sm"
              className="mt-2.5"
              onClick={() => onDeleteFormat(selectedFormat)}
            >
              {t('importWizard.sources.saved.delete')}
            </Button>
          )}
        </div>
      </div>

      <div className="min-w-0">
        <div
          className={cn(
            'grid place-items-center gap-2 rounded-r3 border-[1.5px] border-dashed border-line-strong bg-well px-5 py-[34px] text-center text-table text-ink-3 transition-colors duration-fast',
            isDragging && 'border-dark bg-paper',
            isLoading && 'opacity-60'
          )}
        >
          <Upload className="size-[26px] text-ink-4" aria-hidden />
          <b className="text-body font-650 text-ink">
            {isDragging ? t('importWizard.file.dropActive') : t('importWizard.file.dropTitle')}
          </b>
          <span>{t('importWizard.file.or')}</span>
          <Button type="button" variant="outline" onClick={onChooseFile} loading={isLoading}>
            {t('importWizard.file.choose')}
          </Button>
          <span className="text-micro font-500 text-ink-4">{t('importWizard.file.dropNote')}</span>
        </div>
        <button
          type="button"
          className="mt-3 text-caption font-600 text-ink-3 underline underline-offset-[3px] hover:text-ink disabled:opacity-50"
          onClick={() => void downloadSample()}
          disabled={isLoading}
        >
          {t('importWizard.file.sample')}
        </button>

        <RecentImports batches={batches} formats={formats} onUndo={onUndoBatch} />
      </div>
    </div>
  );
}

/** The guide of a built-in source: how to get the export, its caveats, the columns of the Moony table. */
function SourceGuide({ source }: { source: SourceId }) {
  const { t } = useTranslation('stocks');
  const note = t(`importWizard.sources.${source}.note`, { defaultValue: '' });

  return (
    <>
      <h4 className="m-0 mb-1.5 text-caption font-700 text-ink-2">
        {t(`importWizard.sources.${source}.guideTitle`)}
      </h4>
      <p className="m-0 text-table leading-[1.5] text-ink-2">
        {t(`importWizard.sources.${source}.guide`)}
      </p>
      {source === 'moony' && (
        <dl className="m-0 mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-caption">
          {MOONY_COLUMNS.map((column) => (
            <div key={column} className="contents">
              <dt className="font-700 text-ink-2">
                {t(`importWizard.sources.moony.columns.${column}.label`)}
              </dt>
              <dd className="m-0 text-ink-3">
                {t(`importWizard.sources.moony.columns.${column}.hint`)}
              </dd>
            </div>
          ))}
        </dl>
      )}
      {note && <p className="mb-0 mt-2.5 text-caption leading-[1.5] text-ink-3">{note}</p>}
      {source === 'moony' && (
        <p className="mb-0 mt-2.5 text-caption leading-[1.5] text-ink-3">
          {t('importWizard.sources.moony.excel')}
        </p>
      )}
    </>
  );
}
