import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useAuth } from '@/hooks/use-auth';
import { useLanguage } from '@/i18n/I18nProvider';
import type { CsvPreset } from '@shared/schema';
import { AUTO_PRESET, groupPresetsByCountry, presetLabel } from './import-config';

interface PresetPickerProps {
  presets: readonly CsvPreset[];
  /** `AUTO_PRESET` or a preset id. */
  value: string;
  onChange: (value: string) => void;
  /** A preset was picked by hand but its columns are not in the file. */
  mismatch?: boolean;
  disabled?: boolean;
  /** Show the hint sentence and the bank's export help link under the select. */
  withHint?: boolean;
}

/** Bank preset select with the bank's export help link. */
export function PresetPicker({
  presets,
  value,
  onChange,
  mismatch = false,
  disabled = false,
  withHint = false,
}: PresetPickerProps) {
  const { t } = useTranslation('bank_accounts');
  const { user } = useAuth();
  const { getLocale } = useLanguage();
  const groups = useMemo(
    () => groupPresetsByCountry(presets, user?.country, getLocale()),
    [presets, user?.country, getLocale]
  );
  const selected = presets.find((p) => p.id === value);
  const helpUrl = selected?.exportHelpUrl;

  const openHelp = async () => {
    if (!helpUrl) return;
    try {
      await openUrl(helpUrl);
    } catch (error) {
      console.error('Could not open the export help page:', error);
      toast.error(t('messages.error'), { description: String(error) });
    }
  };

  return (
    <div className="grid gap-1.5">
      <Label htmlFor="csv-preset">{t('csvImport.file.bank')}</Label>
      <Select value={value} onValueChange={onChange} disabled={disabled}>
        <SelectTrigger id="csv-preset">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={AUTO_PRESET}>{t('csvImport.presetAuto')}</SelectItem>
          {groups.map((group) => (
            <SelectGroup key={group.country}>
              <SelectLabel>{group.name}</SelectLabel>
              {group.presets.map((preset) => (
                <SelectItem key={preset.id} value={preset.id}>
                  {presetLabel(preset)}
                </SelectItem>
              ))}
            </SelectGroup>
          ))}
        </SelectContent>
      </Select>
      {(withHint || mismatch) && (
        <p className="text-micro font-500 text-ink-4">
          {mismatch ? t('csvImport.presetMismatch') : t('csvImport.file.bankHint')}{' '}
          {helpUrl && selected && (
            <button
              type="button"
              className="font-600 text-ink-3 underline underline-offset-[3px] hover:text-ink"
              onClick={() => void openHelp()}
            >
              {t('csvImport.howToExport', { bank: selected.bankName })} →
            </button>
          )}
        </p>
      )}
    </div>
  );
}
