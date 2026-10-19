import { useTranslation } from 'react-i18next';
import {
  AlignLeft,
  Banknote,
  Calendar,
  Check,
  Coins,
  Hash,
  Scale,
  User,
  type LucideIcon,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { useFormat } from '@/lib/use-format';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { useFieldText } from '@/mobile/components/import/field-text';
import { ImportNote } from '@/mobile/components/import/ImportNote';
import { PreviewRows } from '@/mobile/components/import/PreviewRows';
import { useHideAmounts } from '@/mobile/hooks/use-hide-amounts';
import type { Recognition } from '@/mobile/hooks/use-recognition';
import {
  maskedSample,
  recognizedFields,
  type ColumnOption,
  type RecognitionField,
} from '@/mobile/lib/recognition';

const FIELD_ICONS: Record<RecognitionField, LucideIcon> = {
  date: Calendar,
  amount: Banknote,
  description: AlignLeft,
  counterparty: User,
  currency: Coins,
  balance: Scale,
  transactionId: Hash,
};

/**
 * "Opravit…" (spec §8.6 rung 3, prototype frame 07): first the fields, as rows with a chevron
 * (what each reads now under its name), then one field's columns as radio options, each with its
 * header and two sample values, and one re-read sample row under them. Never a table of columns
 * to map: one question at a time, answered by a tap on a column. "Použít" in the sheet's footer
 * writes the choice, "Zpět" drops it.
 */
export function CorrectStep({ recognition }: { recognition: Recognition }) {
  const { view } = recognition;
  if (view.kind === 'fields') return <FieldList recognition={recognition} />;
  if (view.kind === 'field') return <FieldChoices recognition={recognition} field={view.field} />;
  return null;
}

/** Every field with what it reads now; a tap opens its columns. */
function FieldList({ recognition }: { recognition: Recognition }) {
  const text = useFieldText();
  const { mapping, inferred } = recognition;
  if (!mapping || !inferred) return null;
  return (
    <EntityRows>
      {recognizedFields(mapping).map(({ field, columns }) => {
        const Icon = FIELD_ICONS[field];
        return (
          <EntityRow
            key={field}
            chip={
              <RowChip square>
                <Icon strokeWidth={1.75} />
              </RowChip>
            }
            title={text.name(field)}
            sub={text.value(field, columns, mapping, inferred, false)}
            chevron
            onClick={() => recognition.openField(field)}
          />
        );
      })}
    </EntityRows>
  );
}

/** One column (or reading of a column) of the file as a radio row: its header and two sample values. */
function ColumnOptionRow({
  option,
  checked,
  onSelect,
}: {
  option: ColumnOption;
  checked: boolean;
  onSelect: () => void;
}) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const { hidden } = useHideAmounts();

  const title =
    option.kind === 'none'
      ? tb('csvImport.columnNone')
      : option.kind === 'order' && option.order
        ? tb(`statementImport.dateOrder.${option.order}`)
        : option.label;
  // Every column shows what it holds; the cells of an amount are masked while amounts are hidden
  const shown = option.samples.map((sample) =>
    hidden ? maskedSample(sample, { money: option.money }) : sample
  );
  // A date order shows its date spelled out, so that "3. dubna" and "4. března" cannot be mistaken
  const dates = (option.dates ?? []).map((day) =>
    fmt.day(day, { day: 'numeric', month: 'long', year: 'numeric' })
  );
  const sub =
    option.kind === 'none'
      ? null
      : option.kind === 'order'
        ? [option.label, ...dates].join(' · ')
        : shown.length > 0
          ? shown.join(' · ')
          : '—';

  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      onClick={onSelect}
      className={cn(
        'mb-2 grid min-h-14 w-full grid-cols-[22px_minmax(0,1fr)] items-center gap-3 rounded-[12px] border bg-paper px-3 py-2.5 text-left [-webkit-touch-callout:none] active:bg-well focus-visible:outline-none focus-visible:shadow-focus',
        checked ? 'border-focus shadow-focus' : 'border-line'
      )}
    >
      <span
        aria-hidden
        className={cn(
          'grid size-[22px] place-items-center rounded-full border-[1.5px]',
          checked ? 'border-ink bg-ink text-ink-inverse' : 'border-line-strong'
        )}
      >
        {checked && <Check className="size-3" strokeWidth={2.5} />}
      </span>
      <span className="min-w-0">
        <span className="block text-m-row text-ink [overflow-wrap:anywhere]">
          {title}
          {option.kind === 'pair' && (
            <span className="ml-1.5 text-m-caption font-medium text-ink-3">
              ({t('import.recognize.pair')})
            </span>
          )}
        </span>
        {sub && <span className="mt-0.5 block truncate text-m-sub text-ink-4 num">{sub}</span>}
      </span>
    </button>
  );
}

/** The columns a field can be read from, and how the first row of the file would look with the one checked. */
function FieldChoices({
  recognition,
  field,
}: {
  recognition: Recognition;
  field: RecognitionField;
}) {
  const { t } = useTranslation('mobile');
  const { options, choice } = recognition;
  const [row] = recognition.rows;

  return (
    <>
      <div role="radiogroup" aria-label={t(`import.recognize.ask.${field}`)}>
        {options.map((option) => (
          <ColumnOptionRow
            key={option.id}
            option={option}
            checked={option.id === choice}
            onSelect={() => recognition.select(option.id)}
          />
        ))}
      </div>
      <p className="mx-0.5 mb-2.5 mt-1.5 text-m-caption text-ink-3">
        {t('import.recognize.sample')}
      </p>
      {!recognition.previewable ? (
        <p className="mx-0.5 my-0 text-m-sub text-ink-3">{t('import.recognize.sampleHint')}</p>
      ) : recognition.previewError ? (
        <ImportNote>{t('import.recognize.previewFailed')}</ImportNote>
      ) : row ? (
        <PreviewRows
          sample
          rows={[row]}
          total={1}
          accountCurrency={recognition.currency}
          updating={recognition.previewUpdating}
        />
      ) : (
        <div aria-hidden className="skeleton mb-2 h-[60px] rounded-r4" />
      )}
    </>
  );
}
