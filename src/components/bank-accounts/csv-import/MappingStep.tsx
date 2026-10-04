import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { FileText, TriangleAlert } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import type { CsvPreset, CsvPreviewResult } from '@shared/schema';
import { ColumnSelect, DescriptionColumnsSelect } from './ColumnSelect';
import { PresetPicker } from './PresetPicker';
import { RawDataPreview } from './RawDataPreview';
import {
  AUTO_PRESET,
  AUTO_VALUE,
  dateFormatChoices,
  dateFormatLabel,
  fileNameFromPath,
  isDateFormatGuess,
  presetLabel,
  suggestionConfidence,
  uniqueHeaders,
  usesDebitCredit,
  type DecimalSetting,
  type MappingRole,
  type MappingState,
} from './import-config';

interface NumberFieldProps {
  id: string;
  label: string;
  value: number;
  min: number;
  onCommit: (value: number) => void;
  disabled?: boolean;
}

/**
 * Whole number that is applied when the user leaves the field or presses
 * Enter, not on every keystroke: each change re-reads the file. A value that
 * is not a number goes back to what it was.
 */
function NumberField({ id, label, value, min, onCommit, disabled }: NumberFieldProps) {
  const [text, setText] = useState(String(value));

  const commit = () => {
    const parsed = Math.trunc(Number(text));
    if (text.trim() === '' || !Number.isFinite(parsed)) {
      setText(String(value));
      return;
    }
    const next = Math.max(min, parsed);
    setText(String(next));
    if (next !== value) onCommit(next);
  };

  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        type="number"
        inputMode="numeric"
        min={min}
        step={1}
        value={text}
        disabled={disabled}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit();
        }}
      />
    </div>
  );
}

interface MappingStepProps {
  filePath: string;
  inspection: CsvPreviewResult;
  mapping: MappingState;
  onMappingChange: (patch: Partial<MappingState>) => void;
  /** Data rows skipped after the header (the file is re-read when it changes). */
  skipRows: number;
  /** Re-read the file with another header line (0-based) or number of skipped rows. */
  onLayoutChange: (layout: { headerRow: number; skipRows: number }) => void;
  /** Changes with every re-read, so the layout fields start from the new values. */
  inspectRevision: number;
  presets: readonly CsvPreset[];
  presetChoice: string;
  /** The preset was recognised from the headers. */
  presetDetected: boolean;
  onPresetChange: (value: string) => void;
  /** The file is being read again (preset or layout changed). */
  isInspecting: boolean;
}

/**
 * Step 2 (prototype csv-import.html): the file bar, then two columns — the
 * required columns and the format on the left, the optional columns on the
 * right — with the detection's confidence next to each label.
 */
export function MappingStep({
  filePath,
  inspection,
  mapping,
  onMappingChange,
  skipRows,
  onLayoutChange,
  inspectRevision,
  presets,
  presetChoice,
  presetDetected,
  onPresetChange,
  isInspecting,
}: MappingStepProps) {
  const { t } = useTranslation('bank_accounts');
  const headers = useMemo(() => uniqueHeaders(inspection.headers), [inspection.headers]);
  const confidence = (role: MappingRole) => suggestionConfidence(inspection, mapping, role);
  const presetMismatch =
    !presetDetected && presetChoice !== AUTO_PRESET && inspection.detectedPresetId == null;
  const preset = presets.find((p) => p.id === presetChoice);

  const pair = usesDebitCredit(mapping);
  const dateFormats = dateFormatChoices(mapping.dateFormat, inspection.dateFormat);
  // The ambiguity was found in the suggested date column, not in one picked by hand.
  const dateFormatIsGuess =
    isDateFormatGuess(inspection, mapping.dateFormat) &&
    mapping.dateColumn === inspection.suggestedMappings?.date?.[0];
  const rowFilter = inspection.suggestedRowFilter;

  const h4 = (text: string) => (
    <h4 className="mb-2.5 mt-1.5 text-caption font-700 text-ink-2">{text}</h4>
  );

  return (
    <div>
      <div className="mb-4 flex items-center gap-3 rounded-r2 border border-line bg-well px-3 py-2.5 text-table text-ink-2">
        <FileText className="size-4 shrink-0 text-ink-3" aria-hidden />
        <span className="min-w-0 truncate">
          <b className="font-650 text-ink">{fileNameFromPath(filePath)}</b> ·{' '}
          {t('csvImport.filebar.rows', { count: inspection.totalRows })}
          {preset && presetDetected && (
            <>
              {' '}
              · {presetLabel(preset)}{' '}
              <Badge variant="gain" className="ml-1.5 align-[1px]">
                {t('csvImport.filebar.detected')}
              </Badge>
            </>
          )}
        </span>
        <span className="ml-auto shrink-0 text-micro font-500 text-ink-4">
          {t('csvImport.filebar.format', {
            delimiter: inspection.delimiter,
            encoding: inspection.encoding,
          })}
        </span>
      </div>

      <div className="grid grid-cols-2 gap-x-6">
        <div className="space-y-4">
          <PresetPicker
            presets={presets}
            value={presetChoice}
            onChange={onPresetChange}
            mismatch={presetMismatch}
            disabled={isInspecting}
          />
          <div>
            {h4(t('csvImport.requiredColumns'))}
            <div className="space-y-4">
              <ColumnSelect
                id="csv-date-column"
                label={t('csvImport.dateColumn')}
                required
                value={mapping.dateColumn}
                headers={headers}
                // The detected format belongs to the old column: let the importer detect it again.
                onChange={(dateColumn) => onMappingChange({ dateColumn, dateFormat: AUTO_VALUE })}
                confidence={confidence('date')}
              />
              <ColumnSelect
                id="csv-amount-column"
                label={t('csvImport.amountColumn')}
                required={!(mapping.debitColumn && mapping.creditColumn)}
                clearable
                value={mapping.amountColumn}
                headers={headers}
                onChange={(amountColumn) => onMappingChange({ amountColumn })}
                confidence={confidence('amount')}
                hint={
                  pair
                    ? t('csvImport.debitCreditHint')
                    : mapping.amountColumn
                      ? t('csvImport.mapping.amountHint')
                      : t('csvImport.amountOrPairHint')
                }
              />
              <div className="grid grid-cols-2 gap-3">
                <ColumnSelect
                  id="csv-debit-column"
                  label={t('csvImport.debitColumn')}
                  optional
                  value={mapping.debitColumn}
                  headers={headers}
                  onChange={(debitColumn) => onMappingChange({ debitColumn })}
                  confidence={confidence('debit')}
                />
                <ColumnSelect
                  id="csv-credit-column"
                  label={t('csvImport.creditColumn')}
                  optional
                  value={mapping.creditColumn}
                  headers={headers}
                  onChange={(creditColumn) => onMappingChange({ creditColumn })}
                  confidence={confidence('credit')}
                />
              </div>
            </div>
          </div>
          <div>
            {h4(t('csvImport.formatSettings'))}
            <div className="grid grid-cols-2 gap-3">
              <div className="grid gap-1.5">
                <Label htmlFor="csv-date-format">{t('csvImport.dateFormat')}</Label>
                <Select
                  value={mapping.dateFormat}
                  onValueChange={(dateFormat) => onMappingChange({ dateFormat })}
                >
                  <SelectTrigger id="csv-date-format">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {dateFormats.map((format) => (
                      <SelectItem key={format} value={format}>
                        {format === AUTO_VALUE
                          ? t('csvImport.dateFormatAuto')
                          : dateFormatLabel(format)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="grid gap-1.5">
                <Label htmlFor="csv-decimal-separator">{t('csvImport.decimalSeparator')}</Label>
                <Select
                  value={mapping.decimalSeparator}
                  onValueChange={(decimalSeparator) =>
                    onMappingChange({ decimalSeparator: decimalSeparator as DecimalSetting })
                  }
                >
                  <SelectTrigger id="csv-decimal-separator">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={AUTO_VALUE}>
                      {t('csvImport.decimalAuto', { separator: inspection.decimalSeparator })}
                    </SelectItem>
                    <SelectItem value=",">{t('csvImport.decimalComma')}</SelectItem>
                    <SelectItem value=".">{t('csvImport.decimalDot')}</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <NumberField
                key={`header-${inspectRevision}`}
                id="csv-header-row"
                label={t('csvImport.headerRow')}
                // Stored 0-based, shown as the line number in a text editor.
                value={inspection.headerRow + 1}
                min={1}
                disabled={isInspecting}
                onCommit={(line) => onLayoutChange({ headerRow: line - 1, skipRows })}
              />
              <NumberField
                key={`skip-${inspectRevision}`}
                id="csv-skip-rows"
                label={t('csvImport.skipRows')}
                value={skipRows}
                min={0}
                disabled={isInspecting}
                onCommit={(rows) =>
                  onLayoutChange({ headerRow: inspection.headerRow, skipRows: rows })
                }
              />
            </div>
            {dateFormatIsGuess && (
              <p className="mt-3 flex items-start gap-2 text-caption font-600 text-loss">
                <TriangleAlert className="mt-px size-3.5 shrink-0" aria-hidden />
                {t('csvImport.dateFormatAmbiguous')}
              </p>
            )}
            <p className="mt-3 text-micro font-500 text-ink-4">{t('csvImport.layoutHelp')}</p>
          </div>
        </div>

        <div className="space-y-4">
          <div>
            {h4(t('csvImport.optionalColumns'))}
            <div className="space-y-4">
              <DescriptionColumnsSelect
                value={mapping.descriptionColumns}
                headers={headers}
                onChange={(descriptionColumns) => onMappingChange({ descriptionColumns })}
                confidence={confidence('description')}
              />
              <ColumnSelect
                id="csv-counterparty-column"
                label={t('csvImport.counterpartyColumn')}
                optional
                value={mapping.counterpartyColumn}
                headers={headers}
                onChange={(counterpartyColumn) => onMappingChange({ counterpartyColumn })}
                confidence={confidence('counterparty')}
              />
              <ColumnSelect
                id="csv-counterparty-iban-column"
                label={t('csvImport.counterpartyIbanColumn')}
                optional
                value={mapping.counterpartyIbanColumn}
                headers={headers}
                onChange={(counterpartyIbanColumn) => onMappingChange({ counterpartyIbanColumn })}
                confidence={confidence('counterparty_iban')}
                hint={t('csvImport.mapping.ibanHint')}
              />
              <div className="grid grid-cols-2 gap-3">
                <ColumnSelect
                  id="csv-balance-column"
                  label={t('csvImport.balanceColumn')}
                  optional
                  value={mapping.balanceColumn}
                  headers={headers}
                  onChange={(balanceColumn) => onMappingChange({ balanceColumn })}
                  confidence={confidence('balance')}
                />
                <ColumnSelect
                  id="csv-transaction-id-column"
                  label={t('csvImport.transactionIdColumn')}
                  optional
                  value={mapping.transactionIdColumn}
                  headers={headers}
                  onChange={(transactionIdColumn) => onMappingChange({ transactionIdColumn })}
                  confidence={confidence('transactionId')}
                />
              </div>
              <ColumnSelect
                id="csv-currency-column"
                label={t('csvImport.currencyColumn')}
                optional
                value={mapping.currencyColumn}
                headers={headers}
                onChange={(currencyColumn) => onMappingChange({ currencyColumn })}
                confidence={confidence('currency')}
              />
            </div>
          </div>
          {rowFilter && (
            <label className="flex cursor-pointer items-start gap-[9px] text-table font-500 text-ink-2">
              <Checkbox
                id="csv-row-filter"
                className="mt-0.5"
                checked={mapping.rowFilterEnabled}
                onCheckedChange={(checked) =>
                  onMappingChange({ rowFilterEnabled: checked === true })
                }
              />
              <span>
                {t('csvImport.rowFilter', { column: rowFilter.column, value: rowFilter.equals })}
                <small className="mt-0.5 block text-micro text-ink-4">
                  {t('csvImport.rowFilterHint')}
                </small>
              </span>
            </label>
          )}
          <RawDataPreview inspection={inspection} />
        </div>
      </div>
    </div>
  );
}
