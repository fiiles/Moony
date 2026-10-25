import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { LearnedCsvFormat } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { useFieldText } from '@/mobile/components/import/field-text';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { Sheet } from '@/mobile/components/Sheet';
import { useLearnedFormats } from '@/mobile/hooks/use-learned-formats';
import { useRetained } from '@/mobile/hooks/use-retained';
import { learnedFormatSub } from '@/mobile/lib/learned-format';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';

/** The longest name the store takes (characters). */
const MAX_NAME = 60;

interface LearnedFormatSheetProps {
  /** The format that was tapped; `null` while the sheet is closed. */
  format: LearnedCsvFormat | null;
  /** The accounts, for the one the format was learned for (it may be gone). */
  accounts: readonly { id: string; name: string; institution?: { name: string } | null }[];
  onOpenChange: (open: boolean) => void;
}

/**
 * A learned CSV format (spec §8.6 rung 4, Nastavení → Import): its name as a field, what it was
 * learned from — the account with its institution, the day, the last import with it and the
 * columns it reads — a quiet note when the bank's own preset now reads this layout (the format
 * waits), and "Smazat formát". Rename is the footer's Uložit (off while the name is unchanged or empty) and
 * closes the sheet; the delete asks first in a confirm sheet that says the next file will be
 * recognized again. The store's own refusals are toasts (`useLearnedFormats`).
 */
export function LearnedFormatSheet({ format, accounts, onOpenChange }: LearnedFormatSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const fieldText = useFieldText();
  const { rename, remove } = useLearnedFormats();
  const fieldId = useId();
  const [value, setValue] = useState('');
  const [confirmOpen, setConfirmOpen] = useState(false);
  // The sheet keeps what it showed while it slides away (and after a delete has taken the row)
  const shown = useRetained(format);

  // Every opening starts from the name the format has now, not from the last attempt; a rename
  // that lands while the sheet is open does not replace what is being typed
  const [seededFor, setSeededFor] = useState<string | null>(null);
  const seed = format?.id ?? null;
  if (seed !== seededFor) {
    setSeededFor(seed);
    if (format) setValue(format.name);
  }

  const trimmed = value.trim();
  const canSave = shown !== null && trimmed !== '' && trimmed !== shown.name && !rename.isPending;

  const day = (unixSeconds: number) => fmt.date(new Date(unixSeconds * 1000));
  const facts = shown ? learnedFormatSub(shown, accounts) : null;

  // "Datum: Datum zaúčtování · Částka: Objem", or the pair with its sides named; a damaged entry
  // without an amount shows what it has
  const columns = shown
    ? [
        t('settings.import.format.field', {
          field: fieldText.name('date'),
          column: shown.mapping.dateColumn,
        }),
        shown.mapping.amountColumn
          ? t('settings.import.format.field', {
              field: fieldText.name('amount'),
              column: shown.mapping.amountColumn,
            })
          : shown.mapping.debitColumn && shown.mapping.creditColumn
            ? fieldText.pairSides({
                debit: shown.mapping.debitColumn,
                credit: shown.mapping.creditColumn,
              })
            : null,
      ]
        .filter(Boolean)
        .join(' · ')
    : '';

  return (
    <>
      <Sheet
        open={format !== null}
        onOpenChange={onOpenChange}
        title={t('settings.import.format.title')}
        footer={
          <>
            <Button
              variant="ghost"
              className="h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] px-[18px] text-m-row"
              onClick={() => onOpenChange(false)}
            >
              {tc('buttons.cancel')}
            </Button>
            <Button
              type="submit"
              form={`${fieldId}-form`}
              className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
              disabled={!canSave}
              loading={rename.isPending}
            >
              {tc('buttons.save')}
            </Button>
          </>
        }
      >
        <form
          id={`${fieldId}-form`}
          noValidate
          onSubmit={(event) => {
            event.preventDefault();
            if (!canSave || !shown) return;
            rename.mutate(
              { id: shown.id, name: trimmed },
              { onSuccess: () => onOpenChange(false) }
            );
          }}
        >
          <label
            htmlFor={fieldId}
            className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2"
          >
            {t('settings.import.format.name')}
          </label>
          <input
            id={fieldId}
            type="text"
            value={value}
            maxLength={MAX_NAME}
            onChange={(event) => setValue(event.target.value)}
            autoComplete="off"
            autoCorrect="off"
            className="h-12 w-full rounded-[10px] border border-line-strong bg-paper px-3.5 text-m-field text-ink transition-[border-color,box-shadow] duration-fast focus:border-focus focus:shadow-focus focus-visible:outline-none"
          />
        </form>

        {shown && facts && (
          <div className="mt-3 divide-y divide-line-soft">
            {facts.accountLabel !== null && (
              <KeyValueRow label={t('settings.import.format.account')} value={facts.accountLabel} />
            )}
            <KeyValueRow label={t('settings.import.format.created')} value={day(facts.created)} />
            <KeyValueRow label={t('settings.import.format.lastUsed')} value={day(facts.lastUsed)} />
            <KeyValueRow stack label={t('settings.import.format.columns')} value={columns} />
          </div>
        )}

        {shown?.shadowedByPresetId && (
          <p className="mx-0.5 mb-0 mt-3 text-m-sub text-ink-3">
            {t('settings.import.format.shadowedNote')}
          </p>
        )}

        <Button
          variant="danger"
          className="mt-3 h-auto min-h-12 w-full whitespace-normal py-2 text-center rounded-[10px] text-m-row"
          onClick={() => setConfirmOpen(true)}
        >
          {t('settings.import.format.delete')}
        </Button>
      </Sheet>

      <ConfirmSheet
        open={confirmOpen && shown !== null}
        onOpenChange={setConfirmOpen}
        title={t('settings.import.format.deleteTitle', { name: shown?.name ?? '' })}
        description={t('settings.import.format.deleteLead')}
        confirmLabel={t('settings.import.format.delete')}
        onConfirm={() => {
          if (!shown) return;
          remove.mutate(shown.id, {
            onSuccess: () => {
              setConfirmOpen(false);
              onOpenChange(false);
            },
          });
        }}
        isPending={remove.isPending}
      />
    </>
  );
}
