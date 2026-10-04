import { useTranslation } from 'react-i18next';
import { ChevronDown } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { NONE_VALUE, type ConfidenceLevel } from './import-config';

export interface Confidence {
  level: ConfidenceLevel;
  percent: number;
}

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
  value: string;
  /** Header cells of the file (blank and repeated ones already removed). */
  headers: readonly string[];
  onChange: (value: string) => void;
  /** Empty means "not used"; shown as "— nepoužít —". */
  optional?: boolean;
  /** Required, but a "—" entry lets the user clear it (amount when debit/credit are used). */
  clearable?: boolean;
  required?: boolean;
  confidence?: Confidence | null;
  hint?: string;
}

/** One column picker: the file's headers, plus "—" for columns that can stay empty. */
export function ColumnSelect({
  id,
  label,
  value,
  headers,
  onChange,
  optional = false,
  clearable = false,
  required = false,
  confidence = null,
  hint,
}: ColumnSelectProps) {
  const { t } = useTranslation('bank_accounts');
  const hasNone = optional || clearable;
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id} className="flex items-center gap-2">
        <span>
          {label}
          {required && <span className="text-ink-4"> *</span>}
        </span>
        <ConfidenceMark confidence={confidence} />
      </Label>
      <Select
        value={value || (optional ? NONE_VALUE : '')}
        onValueChange={(next) => onChange(next === NONE_VALUE ? '' : next)}
      >
        <SelectTrigger id={id}>
          <SelectValue placeholder={t('csvImport.selectColumn')} />
        </SelectTrigger>
        <SelectContent>
          {hasNone && <SelectItem value={NONE_VALUE}>{t('csvImport.columnNone')}</SelectItem>}
          {headers.map((header) => (
            <SelectItem key={header} value={header}>
              {header}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {hint && <p className="text-micro font-500 text-ink-4">{hint}</p>}
    </div>
  );
}

interface DescriptionColumnsSelectProps {
  value: readonly string[];
  headers: readonly string[];
  onChange: (value: string[]) => void;
  confidence?: Confidence | null;
}

/** Several columns can feed the description: a popover with one checkbox per header. */
export function DescriptionColumnsSelect({
  value,
  headers,
  onChange,
  confidence = null,
}: DescriptionColumnsSelectProps) {
  const { t } = useTranslation('bank_accounts');
  const summary =
    value.length === 0
      ? t('csvImport.selectColumns')
      : value.length === 1
        ? value[0]
        : `${value[0]} +${value.length - 1}`;

  return (
    <div className="grid gap-1.5">
      <Label className="flex items-center gap-2">
        <span>{t('csvImport.descriptionColumns')}</span>
        <ConfidenceMark confidence={confidence} />
      </Label>
      <Popover>
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="outline"
            className="h-9 w-full justify-between overflow-hidden px-3 text-left font-500"
          >
            <span className="min-w-0 flex-1 truncate">{summary}</span>
            <ChevronDown className="ml-2 size-3.5 shrink-0 text-ink-4" />
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-[300px] p-0" align="start">
          <div className="max-h-[200px] overflow-y-auto p-2" onWheel={(e) => e.stopPropagation()}>
            {headers.map((header) => (
              <label
                key={header}
                className="flex cursor-pointer items-center gap-3 rounded-r1 px-2 py-1.5 text-table text-ink-2 hover:bg-well"
              >
                <Checkbox
                  checked={value.includes(header)}
                  onCheckedChange={(checked) =>
                    onChange(checked ? [...value, header] : value.filter((c) => c !== header))
                  }
                />
                {header}
              </label>
            ))}
          </div>
        </PopoverContent>
      </Popover>
      <p className="text-micro font-500 text-ink-4">{t('csvImport.descriptionColumnsHelp')}</p>
    </div>
  );
}
