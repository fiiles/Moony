import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { TriangleAlert } from 'lucide-react';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Segmented } from '@/components/ui/segmented';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import type { StockCsvInspection, StockCurrencyMode, StockDirectionMode } from '@shared/schema';
import { ColumnSelect } from './ColumnSelect';
import { TypeValuesTable } from './TypeValuesTable';
import {
  AUTO_VALUE,
  MAX_FORMAT_NAME_LENGTH,
  dateFormatChoices,
  dateFormatLabel,
  directionModePatch,
  hasTradeValue,
  incompleteFields,
  isDateFormatGuess,
  isCurrencyCode,
  setTypeValueAction,
  suggestionConfidence,
  typeColumnPatch,
  typeValueRows,
  type ColumnOption,
  type DecimalSetting,
  type MappingState,
} from './import-config';

/** What the user decides about remembering the mapping for the next file. */
export interface RememberFormat {
  enabled: boolean;
  name: string;
}

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
 * Enter, not on every keystroke: each change reads the file again. A value that
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

interface MappingFormProps {
  inspection: StockCsvInspection;
  mapping: MappingState;
  options: readonly ColumnOption[];
  onChange: (patch: Partial<MappingState>) => void;
  /** Data rows skipped after the header (the file is read again when it changes). */
  skipRows: number;
  /** Read the file again with another header line (0-based) or number of skipped rows. */
  onLayoutChange: (layout: { headerRow: number; skipRows: number }) => void;
  /** Changes with every re-read, so the layout fields start from the new values. */
  inspectRevision: number;
  isInspecting: boolean;
  /** Null when the mapping is already a remembered format. */
  remember: RememberFormat | null;
  onRememberChange: (patch: Partial<RememberFormat>) => void;
}

/**
 * The full mapping form (prototype csv-import.html, step 2): the required
 * columns, the direction with its value table and the format on the left, the
 * optional columns and "remember this format" on the right, the detection's
 * confidence next to each label.
 */
export function MappingForm({
  inspection,
  mapping,
  options,
  onChange,
  skipRows,
  onLayoutChange,
  inspectRevision,
  isInspecting,
  remember,
  onRememberChange,
}: MappingFormProps) {
  const { t } = useTranslation('stocks');
  const confidence = (role: Parameters<typeof suggestionConfidence>[2]) =>
    suggestionConfidence(inspection, mapping, role);
  const missing = incompleteFields(mapping);
  const dateFormats = dateFormatChoices(mapping.dateFormat, inspection.dateFormat);
  const dateFormatIsGuess = isDateFormatGuess(inspection, mapping);
  // The mapping lists every value; where the column was counted the file's own values lead.
  const typeRows = typeValueRows(
    mapping.typeValues,
    inspection.columnValues.find((entry) => entry.column === mapping.typeColumn)?.values
  );
  const nameEmpty = remember?.enabled === true && remember.name.trim() === '';

  const h4 = (text: string) => (
    <h4 className="mb-2.5 mt-1.5 text-caption font-700 text-ink-2">{text}</h4>
  );

  const currencyModes: { value: StockCurrencyMode; label: string }[] = [
    { value: 'column', label: t('importWizard.mapping.currencyMode.column') },
    { value: 'fixed', label: t('importWizard.mapping.currencyMode.fixed') },
    { value: 'instrument', label: t('importWizard.mapping.currencyMode.instrument') },
  ];
  const directionModes: { value: StockDirectionMode; label: string }[] = [
    { value: 'typeColumn', label: t('importWizard.mapping.directionMode.typeColumn') },
    { value: 'quantitySign', label: t('importWizard.mapping.directionMode.quantitySign') },
  ];

  return (
    <div className="grid grid-cols-2 gap-x-6">
      <div className="min-w-0 space-y-4">
        <div>
          {h4(t('importWizard.mapping.required'))}
          <div className="space-y-4">
            <ColumnSelect
              id="stock-import-date"
              label={t('importWizard.roles.date')}
              required
              value={mapping.dateColumn}
              options={options}
              onChange={(dateColumn) => onChange({ dateColumn })}
              confidence={confidence('date')}
            />
            <div>
              <div className="grid grid-cols-2 gap-3">
                <ColumnSelect
                  id="stock-import-symbol"
                  label={t('importWizard.roles.symbol')}
                  optional
                  value={mapping.symbolColumn}
                  options={options}
                  onChange={(symbolColumn) => onChange({ symbolColumn })}
                  confidence={confidence('symbol')}
                />
                <ColumnSelect
                  id="stock-import-isin"
                  label={t('importWizard.roles.isin')}
                  optional
                  value={mapping.isinColumn}
                  options={options}
                  onChange={(isinColumn) => onChange({ isinColumn })}
                  confidence={confidence('isin')}
                />
              </div>
              <p className="mb-0 mt-1.5 text-micro font-500 text-ink-4">
                {t('importWizard.mapping.symbolOrIsin')}
              </p>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <ColumnSelect
                id="stock-import-quantity"
                label={t('importWizard.roles.quantity')}
                required
                value={mapping.quantityColumn}
                options={options}
                onChange={(quantityColumn) => onChange({ quantityColumn })}
                confidence={confidence('quantity')}
              />
              <ColumnSelect
                id="stock-import-price"
                label={t('importWizard.roles.price')}
                required
                value={mapping.priceColumn}
                options={options}
                onChange={(priceColumn) => onChange({ priceColumn })}
                confidence={confidence('price')}
              />
            </div>
            <div className="grid gap-1.5">
              <Label>
                <span>
                  {t('importWizard.roles.currency')}
                  <span className="text-ink-4"> *</span>
                </span>
              </Label>
              <Segmented
                value={mapping.currencyMode}
                onValueChange={(currencyMode) => onChange({ currencyMode })}
                options={currencyModes}
                fullWidth
                aria-label={t('importWizard.roles.currency')}
              />
              {mapping.currencyMode === 'column' && (
                <ColumnSelect
                  id="stock-import-currency"
                  label={t('importWizard.mapping.currencyColumn')}
                  value={mapping.currencyColumn}
                  options={options}
                  onChange={(currencyColumn) => onChange({ currencyColumn })}
                  confidence={confidence('currency')}
                />
              )}
              {mapping.currencyMode === 'fixed' && (
                <div className="grid gap-1.5">
                  <CurrencyCombobox
                    value={isCurrencyCode(mapping.fixedCurrency) ? mapping.fixedCurrency : ''}
                    onChange={(fixedCurrency) => onChange({ fixedCurrency })}
                    aria-label={t('importWizard.mapping.currencyFixed')}
                  />
                  <p className="m-0 text-micro font-500 text-ink-4">
                    {t('importWizard.mapping.currencyFixedHint')}
                  </p>
                </div>
              )}
              {mapping.currencyMode === 'instrument' && (
                <p className="m-0 text-micro font-500 text-ink-4">
                  {t('importWizard.mapping.currencyInstrumentHint')}
                </p>
              )}
            </div>
          </div>
        </div>

        <div>
          {h4(t('importWizard.mapping.direction'))}
          <div className="space-y-3">
            <Segmented
              value={mapping.directionMode}
              onValueChange={(mode) => onChange(directionModePatch(inspection, mapping, mode))}
              options={directionModes}
              fullWidth
              aria-label={t('importWizard.mapping.direction')}
            />
            {mapping.directionMode === 'quantitySign' ? (
              <p className="m-0 text-micro font-500 text-ink-4">
                {t('importWizard.mapping.directionSignHint')}
              </p>
            ) : (
              <>
                <ColumnSelect
                  id="stock-import-type"
                  label={t('importWizard.mapping.typeColumn')}
                  required
                  value={mapping.typeColumn}
                  options={options}
                  onChange={(column) => onChange(typeColumnPatch(inspection, column))}
                  confidence={confidence('type')}
                />
                {mapping.typeColumn != null && (
                  <TypeValuesTable
                    rows={typeRows}
                    hasTradeValue={hasTradeValue(mapping.typeValues)}
                    onChange={(entryValue, action) =>
                      onChange({
                        typeValues: setTypeValueAction(mapping.typeValues, entryValue, action),
                      })
                    }
                    commentDecidesDirection={mapping.transforms.xtbComment === true}
                  />
                )}
              </>
            )}
          </div>
        </div>
      </div>

      <div className="min-w-0 space-y-4">
        <div>
          {h4(t('importWizard.mapping.optional'))}
          <div className="space-y-4">
            <ColumnSelect
              id="stock-import-name"
              label={t('importWizard.roles.name')}
              optional
              value={mapping.nameColumn}
              options={options}
              onChange={(nameColumn) => onChange({ nameColumn })}
              confidence={confidence('name')}
            />
            <ColumnSelect
              id="stock-import-external-id"
              label={t('importWizard.roles.externalId')}
              optional
              value={mapping.externalIdColumn}
              options={options}
              onChange={(externalIdColumn) => onChange({ externalIdColumn })}
              confidence={confidence('externalId')}
              hint={t('importWizard.mapping.externalIdHint')}
            />
          </div>
        </div>

        <div>
          {h4(t('importWizard.mapping.format'))}
          <div className="grid grid-cols-2 gap-3">
            <div className="grid gap-1.5">
              <Label htmlFor="stock-import-date-format">
                <span>
                  {t('importWizard.mapping.dateFormat')}
                  <span className="text-ink-4"> *</span>
                </span>
              </Label>
              <Select
                value={mapping.dateFormat}
                onValueChange={(dateFormat) => onChange({ dateFormat })}
              >
                <SelectTrigger id="stock-import-date-format">
                  <SelectValue placeholder={t('importWizard.mapping.dateFormatChoose')}>
                    {mapping.dateFormat ? dateFormatLabel(mapping.dateFormat) : undefined}
                  </SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {dateFormats.map((format) => (
                    <SelectItem key={format} value={format}>
                      {dateFormatLabel(format)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="stock-import-decimal">
                {t('importWizard.mapping.decimalSeparator')}
              </Label>
              <Select
                value={mapping.decimalSeparator}
                onValueChange={(decimalSeparator) =>
                  onChange({ decimalSeparator: decimalSeparator as DecimalSetting })
                }
              >
                <SelectTrigger id="stock-import-decimal">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={AUTO_VALUE}>
                    {t('importWizard.mapping.decimalAuto', {
                      separator: inspection.decimalSeparator,
                    })}
                  </SelectItem>
                  <SelectItem value=",">{t('importWizard.mapping.decimalComma')}</SelectItem>
                  <SelectItem value=".">{t('importWizard.mapping.decimalDot')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <NumberField
              key={`header-${inspectRevision}`}
              id="stock-import-header-row"
              label={t('importWizard.mapping.headerRow')}
              // Stored 0-based, shown as the line number in a text editor.
              value={inspection.headerRow + 1}
              min={1}
              disabled={isInspecting}
              onCommit={(line) => onLayoutChange({ headerRow: line - 1, skipRows })}
            />
            <NumberField
              key={`skip-${inspectRevision}`}
              id="stock-import-skip-rows"
              label={t('importWizard.mapping.skipRows')}
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
              {t('importWizard.mapping.dateFormatAmbiguous')}
            </p>
          )}
          <p className="mb-0 mt-3 text-micro font-500 text-ink-4">
            {t('importWizard.mapping.layoutHelp')}
          </p>
        </div>

        {remember && (
          <div className="rounded-r2 border border-line px-3 py-3">
            <label className="flex cursor-pointer items-start gap-[9px] text-table font-500 text-ink-2">
              <Checkbox
                id="stock-import-remember"
                className="mt-0.5"
                checked={remember.enabled}
                onCheckedChange={(checked) => onRememberChange({ enabled: checked === true })}
              />
              <span>
                {t('importWizard.mapping.remember')}
                <small className="mt-0.5 block text-micro text-ink-4">
                  {t('importWizard.mapping.rememberHint')}
                </small>
              </span>
            </label>
            {remember.enabled && (
              <div className="mt-3 grid gap-1.5">
                <Label htmlFor="stock-import-format-name">
                  {t('importWizard.mapping.formatName')}
                </Label>
                <Input
                  id="stock-import-format-name"
                  value={remember.name}
                  maxLength={MAX_FORMAT_NAME_LENGTH}
                  autoComplete="off"
                  placeholder={t('importWizard.mapping.formatNamePlaceholder')}
                  aria-invalid={nameEmpty || undefined}
                  onChange={(e) => onRememberChange({ name: e.target.value })}
                />
                {nameEmpty && (
                  <p className="m-0 text-micro font-600 text-loss">
                    {t('importWizard.mapping.formatNameRequired')}
                  </p>
                )}
              </div>
            )}
          </div>
        )}

        {missing.length > 0 && (
          <p className="m-0 text-caption font-600 text-ink-3">
            {t('importWizard.mapping.missing', {
              fields: missing.map((field) => t(`importWizard.mapping.field.${field}`)).join(', '),
            })}
          </p>
        )}
      </div>
    </div>
  );
}
