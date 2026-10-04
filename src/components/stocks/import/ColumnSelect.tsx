import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  columnToSelectValue,
  optionLabel,
  selectValueToColumn,
  type ColumnOption,
  type Confidence,
} from './import-config';

/** The detection's confidence as a short bar with the percentage (prototype `.conf`). */
export function ConfidenceMark({ confidence }: { confidence: Confidence | null }) {
  if (!confidence) return null;
  return (
    <span className="ml-auto inline-flex items-center gap-1 text-micro font-600 text-ink-4">
      <i className="relative block h-[3px] w-7 overflow-hidden rounded-[2px] bg-well-3">
        <i
          className="absolute inset-y-0 left-0 block bg-gain"
          style={{ width: `${confidence.percent}%` }}
        />
      </i>
      {confidence.percent} %
    </span>
  );
}

interface ColumnSelectProps {
  id: string;
  label: string;
  /** 0-based column, null = none. */
  value: number | null;
  /** Every column of the file. */
  options: readonly ColumnOption[];
  onChange: (value: number | null) => void;
  /** Can stay empty: a "— nepoužít —" entry clears it. */
  optional?: boolean;
  required?: boolean;
  confidence?: Confidence | null;
  hint?: ReactNode;
  disabled?: boolean;
  /** Shows a few values of each column next to its name in the list (default on). */
  withExamples?: boolean;
}

/**
 * One column picker. The list shows each column with a few values from the file,
 * so "Time" and "Settlement date" can be told apart; under the field the values
 * of the chosen column confirm the pick.
 */
export function ColumnSelect({
  id,
  label,
  value,
  options,
  onChange,
  optional = false,
  required = false,
  confidence = null,
  hint,
  disabled = false,
  withExamples = true,
}: ColumnSelectProps) {
  const { t } = useTranslation('stocks');
  const selected = options.find((option) => option.index === value);
  const example = selected?.example;

  return (
    <div className="grid min-w-0 gap-1.5">
      <Label htmlFor={id} className="flex items-center gap-2">
        <span>
          {label}
          {required && <span className="text-ink-4"> *</span>}
        </span>
        <ConfidenceMark confidence={confidence} />
      </Label>
      <Select
        value={value == null ? (optional ? columnToSelectValue(null) : '') : String(value)}
        onValueChange={(next) => onChange(selectValueToColumn(next))}
        disabled={disabled}
      >
        <SelectTrigger id={id}>
          <SelectValue placeholder={t('importWizard.mapping.selectColumn')}>
            {optionLabel(options, value)}
          </SelectValue>
        </SelectTrigger>
        <SelectContent>
          {optional && (
            <SelectItem value={columnToSelectValue(null)}>
              {t('importWizard.mapping.columnNone')}
            </SelectItem>
          )}
          {options.map((option) => (
            <SelectItem key={option.index} value={String(option.index)}>
              <span className="flex min-w-0 items-baseline gap-2">
                <span className="truncate">{option.label}</span>
                {withExamples && option.example && (
                  <span className="max-w-[200px] truncate text-micro font-500 text-ink-4">
                    {option.example}
                  </span>
                )}
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {hint ? (
        <p className="text-micro font-500 text-ink-4">{hint}</p>
      ) : example ? (
        <p className="truncate text-micro font-500 text-ink-4">
          {t('importWizard.mapping.example', { values: example })}
        </p>
      ) : null}
    </div>
  );
}
