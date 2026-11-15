import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { PICKER_FRAME, PICKER_INPUT, openPicker } from '@/mobile/components/field-styles';
import { isoDayToUtcSeconds } from '@/mobile/lib/picker-value';

interface DateFieldProps {
  label: string;
  /** `YYYY-MM-DD`, or empty. */
  value: string;
  onChange: (value: string) => void;
  /** The latest day the picker offers (`YYYY-MM-DD`): a valuation or a cost is never from the future. */
  max?: string;
  /** Under the field, in `loss`; also marks the field invalid. */
  error?: string;
  /** What an empty field says ("Vyberte den" by default). */
  emptyText?: string;
}

/**
 * A 48 pt date field of a phone form (README §13, `.m-field__i`) with its label above. The system's
 * date picker lies transparent over the field, so a tap opens it, while the field shows the day
 * written by the app's formatter in the app's language (`fmt.day`), not the native input's text,
 * which follows the system language and shows dashes when empty (audit B8); an empty field says
 * `emptyText` in `ink-5`. The value is the ISO day the input speaks, so it never goes through a
 * `Date` of the local time zone; a day the backend keeps is its UTC-midnight second (`isoToUtcDay`).
 */
export function DateField({ label, value, onChange, max, error, emptyText }: DateFieldProps) {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const id = useId();
  const errorId = `${id}-error`;
  const seconds = isoDayToUtcSeconds(value);
  return (
    <div className="min-w-0">
      <label htmlFor={id} className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2">
        {label}
      </label>
      <div className="relative">
        <input
          id={id}
          type="date"
          value={value}
          max={max}
          onChange={(event) => onChange(event.target.value)}
          onClick={(event) => openPicker(event.currentTarget)}
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? errorId : undefined}
          className={PICKER_INPUT}
        />
        <div aria-hidden className={PICKER_FRAME}>
          <span className={cn('truncate num', seconds === null ? 'text-ink-5' : 'text-ink')}>
            {seconds === null ? (emptyText ?? t('fields.pickDay')) : fmt.day(seconds)}
          </span>
        </div>
      </div>
      {error && (
        <p
          id={errorId}
          role="alert"
          className="mx-0.5 mb-0 mt-1.5 text-m-sub font-semibold text-loss"
        >
          {error}
        </p>
      )}
    </div>
  );
}
