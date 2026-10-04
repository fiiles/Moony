import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { save } from '@tauri-apps/plugin-dialog';
import { writeTextFile } from '@tauri-apps/plugin-fs';
import { toast } from 'sonner';
import { Upload } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import type { CsvPreset } from '@shared/schema';
import { PresetPicker } from './PresetPicker';
import { SAMPLE_CSV, sortPresets } from './import-config';

interface FileSelectStepProps {
  presets: readonly CsvPreset[];
  presetChoice: string;
  onPresetChange: (value: string) => void;
  /** Open the native file dialog. */
  onChooseFile: () => void;
  /** A file is being read. */
  isLoading: boolean;
  /** A file is being dragged over the window. */
  isDragging: boolean;
}

/**
 * Step 1 (prototype csv-import.html): the bank preset with its export help on
 * the left, the drop zone on the right, the sample file as a quiet link.
 */
export function FileSelectStep({
  presets,
  presetChoice,
  onPresetChange,
  onChooseFile,
  isLoading,
  isDragging,
}: FileSelectStepProps) {
  const { t } = useTranslation('bank_accounts');
  const bankNames = useMemo(() => sortPresets(presets).map((preset) => preset.bankName), [presets]);

  const downloadSample = async () => {
    try {
      const path = await save({
        defaultPath: 'moony-sample-statement.csv',
        filters: [{ name: 'CSV', extensions: ['csv'] }],
      });
      if (!path) return;
      await writeTextFile(path, SAMPLE_CSV);
      toast(t('csvImport.sampleSaved'));
    } catch (error) {
      console.error('Could not save the sample CSV:', error);
      toast.error(t('csvImport.sampleFailed'), { description: String(error) });
    }
  };

  return (
    <div className="grid grid-cols-[1fr_1.3fr] gap-6">
      <div>
        <PresetPicker
          presets={presets}
          value={presetChoice}
          onChange={onPresetChange}
          disabled={isLoading}
          withHint
        />
        <p className="mb-0 mt-4 text-caption text-ink-3">
          {bankNames.length > 0
            ? t('csvImport.file.supported', { banks: bankNames.join(', ') })
            : t('csvImport.file.supportedNone')}
        </p>
        <button
          type="button"
          className="mt-2.5 text-caption font-600 text-ink-3 underline underline-offset-[3px] hover:text-ink disabled:opacity-50"
          onClick={() => void downloadSample()}
          disabled={isLoading}
        >
          {t('csvImport.file.sample')}
        </button>
      </div>
      <div
        className={cn(
          'grid place-items-center gap-2 rounded-r3 border-[1.5px] border-dashed border-line-strong bg-well px-5 py-[34px] text-center text-table text-ink-3 transition-colors duration-fast',
          isDragging && 'border-dark bg-paper',
          isLoading && 'opacity-60'
        )}
      >
        <Upload className="size-[26px] text-ink-4" aria-hidden />
        <b className="text-body font-650 text-ink">
          {isDragging ? t('csvImport.file.dropActive') : t('csvImport.file.dropTitle')}
        </b>
        <span>{t('csvImport.file.or')}</span>
        <Button type="button" variant="outline" onClick={onChooseFile} loading={isLoading}>
          {t('csvImport.file.choose')}
        </Button>
        <span className="text-micro font-500 text-ink-4">{t('csvImport.file.dropNote')}</span>
      </div>
    </div>
  );
}
