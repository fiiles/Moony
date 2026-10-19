import { useTranslation } from 'react-i18next';
import { Check, Minus, TriangleAlert } from 'lucide-react';
import { cn } from '@/lib/utils';
import { FileBar } from '@/mobile/components/import/FileBar';
import { useFieldText } from '@/mobile/components/import/field-text';
import { ImportNote } from '@/mobile/components/import/ImportNote';
import { PreviewRows } from '@/mobile/components/import/PreviewRows';
import type { Recognition } from '@/mobile/hooks/use-recognition';
import { SAMPLE_ROWS, recognizedFields, signCheckFor } from '@/mobile/lib/recognition';

/** Three rows' worth of grey while the file is being read with the columns it was given. */
export function SampleSkeleton() {
  return (
    <div aria-hidden className="mb-3.5 overflow-hidden rounded-r4 border border-line bg-paper">
      {[0, 1, 2].map((i) => (
        <div key={i} className="flex min-h-[60px] items-center gap-3 px-3.5 py-2.5">
          <div className="skeleton size-9 flex-none rounded-full" />
          <div className="min-w-0 flex-1">
            <div className="skeleton h-3.5 w-2/5" />
            <div className="skeleton mt-2 h-3 w-3/5" />
          </div>
        </div>
      ))}
    </div>
  );
}

/**
 * "Takhle tomu rozumím" (spec §8.6 rung 3, README §13 "Import recognition"): the file bar with
 * what the reader found of the file and that its format is not known yet, three real rows of the
 * file drawn as Moony rows, and the list of what was recognized — a check and the columns for
 * every field that is read, a minus and what it costs for one that is not (the date and the
 * amount are always read here; a file without them opens on their correction instead). The
 * amount line says whether the balance proved its sign; an amount that looks inverted is a
 * warning to check the rows above. No mapping grid, ever: the buttons are in the sheet's footer.
 */
export function RecognizeStep({
  fileName,
  recognition,
}: {
  fileName: string;
  recognition: Recognition;
}) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const text = useFieldText();
  const { csv, inferred, mapping, rows } = recognition;
  if (!csv || !inferred || !mapping) return null;

  const meta = [
    tb('csvImport.filebar.rows', { count: csv.totalRows }),
    tb('csvImport.filebar.format', {
      delimiter: csv.delimiter === '\t' ? t('import.recognize.tab') : csv.delimiter,
      encoding: csv.encoding,
    }),
    t('import.recognize.unknownFormat'),
  ].join(' · ');
  const signCheck = signCheckFor(mapping, inferred);
  const full = recognition.previewLoading || rows.length >= SAMPLE_ROWS;

  return (
    <div>
      <FileBar name={fileName} meta={meta} wrapMeta />
      <h3 className="mx-1 mb-0.5 mt-1 text-m-section text-ink">{t('import.recognize.title')}</h3>
      <p className="mx-1 mb-2 mt-0 text-m-sub text-ink-3">
        {full ? t('import.recognize.caption') : t('import.recognize.captionFew')}
      </p>

      {recognition.previewError ? (
        <ImportNote>{t('import.recognize.previewFailed')}</ImportNote>
      ) : rows.length === 0 && recognition.previewLoading ? (
        <SampleSkeleton />
      ) : rows.length > 0 ? (
        <PreviewRows
          sample
          rows={rows}
          total={rows.length}
          accountCurrency={recognition.currency}
          updating={recognition.previewUpdating}
        />
      ) : null}

      <ul className="m-0 mt-3 list-none p-0">
        {recognizedFields(mapping).map(({ field, columns }) => {
          const read = columns.length > 0;
          return (
            <li
              key={field}
              className={cn(
                'grid grid-cols-[20px_minmax(92px,34%)_minmax(0,1fr)] items-start gap-2 border-t border-line-soft py-[9px] text-[13px] font-450 leading-[1.4] first:border-t-0',
                read ? 'text-ink-2' : 'text-ink-4'
              )}
            >
              {read ? (
                <Check aria-hidden className="mt-0.5 size-4 flex-none text-gain" strokeWidth={2} />
              ) : (
                <Minus aria-hidden className="mt-0.5 size-4 flex-none text-ink-5" strokeWidth={2} />
              )}
              <b
                className={cn(
                  'font-semibold [overflow-wrap:anywhere]',
                  read ? 'text-ink' : 'text-ink-4'
                )}
              >
                {text.name(field)}
              </b>
              <span className="min-w-0 text-m-sub text-ink-3 [overflow-wrap:anywhere]">
                {text.value(field, columns, mapping, inferred)}
                {field === 'amount' && read && signCheck === 'verified' && (
                  <> · {t('import.recognize.signVerified')}</>
                )}
                {field === 'amount' && read && signCheck === 'inverted' && (
                  <span className="mt-0.5 flex items-start gap-1.5 text-loss">
                    <TriangleAlert
                      aria-hidden
                      className="mt-px size-3.5 flex-none"
                      strokeWidth={1.75}
                    />
                    <span>{t('import.recognize.signInverted')}</span>
                  </span>
                )}
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
