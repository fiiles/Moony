import { useMemo, useState } from 'react';
import type { CsvPreviewResult, InferredColumns } from '@shared/schema';
import {
  buildImportConfig,
  type MappingState,
} from '@/components/bank-accounts/csv-import/import-config';
import { useCsvPreview } from '@/components/bank-accounts/csv-import/use-csv-preview';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import {
  SAMPLE_ROWS,
  afterStartApply,
  applyOption,
  canImport,
  columnOptions,
  mappingFromInferred,
  recognitionStart,
  selectedOption,
  signsBlockImport,
  type RecognitionField,
  type RecognitionStep,
} from '@/mobile/lib/recognition';

/** What the recognition shows: the three rows and the list, the list of fields, or one field's columns. */
export type RecognitionView =
  | { kind: 'overview' }
  | { kind: 'fields' }
  /** `start`: the recognition opened this question itself (a field it could not tell), not the person. */
  | { kind: 'field'; field: RecognitionField; start: boolean };

/** The file the recognition works on. */
interface Session {
  csv: CsvPreviewResult;
  path: string;
  inferred: InferredColumns;
  /** The account the three sample rows are previewed against (they only need one to exist). */
  anchorId: string;
}

interface UseRecognitionArgs {
  /** The import sheet is open and in the `recognize` or `correct` stage. */
  active: boolean;
  /** The recognition moves the sheet between its two stages. */
  goTo: (stage: 'recognize' | 'correct') => void;
  /** The field the recognition just asked is still empty after its own question: no statement. */
  onUnreadable: () => void;
  /**
   * A question opened, closed or answered: whatever the sheet was waiting for since the person
   * last tapped "Vypadá to správně" is out of date and must not move the sheet on.
   */
  onNavigate: () => void;
}

/**
 * The guided recognition of an unknown CSV (spec §8.6 rung 3): what the content inference found,
 * the three real rows rendered with it, and the per-field correction. `begin` takes the
 * inference and says which stage opens — the correction of a date or an amount the inference
 * could not tell (and of a day and month order that is a guess), else the recognition itself.
 * A field's correction is a list of the file's columns; the choice is shown on one re-previewed
 * row (`previewCsvImport`, three rows) before "Použít" writes it into the mapping.
 */
export function useRecognition({ active, goTo, onUnreadable, onNavigate }: UseRecognitionArgs) {
  const { accounts } = useBankAccounts();
  const [session, setSession] = useState<Session | null>(null);
  /** What the sheet will import: the inference with the corrections applied. */
  const [mapping, setMapping] = useState<MappingState | null>(null);
  const [view, setView] = useState<RecognitionView>({ kind: 'overview' });
  /** The option of the open field that is checked; `null` until one is. */
  const [choice, setChoice] = useState<string | null>(null);
  /** The person saw the question of the dates' day and month order. */
  const [orderConfirmed, setOrderConfirmed] = useState(false);

  const options = useMemo(
    () =>
      view.kind === 'field' && session
        ? columnOptions(view.field, session.csv, session.inferred, mapping ?? undefined)
        : [],
    [view, session, mapping]
  );
  const chosen = options.find((option) => option.id === choice) ?? null;
  /** The mapping the open field would give: its checked option over the current one. */
  const pending = useMemo(
    () => (mapping && chosen ? applyOption(mapping, chosen) : mapping),
    [mapping, chosen]
  );
  const shown = view.kind === 'field' ? pending : mapping;

  const config = useMemo(
    () =>
      session && shown && canImport(shown)
        ? {
            ...buildImportConfig(shown, session.csv, 0),
            previewOffset: 0,
            previewLimit: SAMPLE_ROWS,
          }
        : null,
    [session, shown]
  );
  const {
    preview,
    error: previewError,
    isUpdating,
    isLoadingFirst,
  } = useCsvPreview({
    accountId: session?.anchorId ?? '',
    filePath: session?.path ?? '',
    config,
    enabled: active && session !== null && view.kind !== 'fields',
  });

  /** Opens a step of the recognition; says which stage of the sheet it is. */
  const show = (
    step: RecognitionStep,
    current: MappingState,
    file: Session,
    start: boolean
  ): 'recognize' | 'correct' => {
    if (step.stage === 'recognize') {
      setView({ kind: 'overview' });
      setChoice(null);
      return 'recognize';
    }
    setView({ kind: 'field', field: step.field, start });
    setChoice(selectedOption(columnOptions(step.field, file.csv, file.inferred, current), current));
    return 'correct';
  };

  /**
   * The inference of a file is in: where the sheet goes next. With a `seed` (the mapping of a
   * learned format the person wants to change) the recognition starts from it, on the rows rather
   * than the questions, and its day and month order counts as confirmed — it was when the format
   * was learned. The inference still gives the options and the sign check.
   */
  const begin = (file: Session, seed?: MappingState): 'recognize' | 'correct' => {
    const first = seed ?? mappingFromInferred(file.inferred, file.csv);
    const seeded = seed !== undefined;
    setSession(file);
    setMapping(first);
    setOrderConfirmed(seeded);
    return show(recognitionStart(first, file.inferred, seeded), first, file, true);
  };

  /** A new read starts: nothing of the previous file is kept. */
  const reset = () => {
    setSession(null);
    setMapping(null);
    setView({ kind: 'overview' });
    setChoice(null);
    setOrderConfirmed(false);
  };

  /** "Opravit…": the list of fields. */
  const openFields = () => {
    onNavigate();
    setView({ kind: 'fields' });
    setChoice(null);
    goTo('correct');
  };

  const openField = (field: RecognitionField) => {
    if (!session || !mapping) return;
    onNavigate();
    show({ stage: 'correct', field }, mapping, session, false);
  };

  /** "Zpět": from a field to the list, from the list to the rows. */
  const back = () => {
    onNavigate();
    if (view.kind === 'field' && !view.start) {
      setView({ kind: 'fields' });
      setChoice(null);
    } else {
      setView({ kind: 'overview' });
      setChoice(null);
      goTo('recognize');
    }
  };

  /** "Použít": the checked option goes into the mapping. */
  const apply = () => {
    if (!session || !mapping || view.kind !== 'field' || !chosen) return;
    onNavigate();
    const next = applyOption(mapping, chosen);
    setMapping(next);
    const confirmed = orderConfirmed || view.field === 'date';
    setOrderConfirmed(confirmed);
    if (!view.start) {
      setView({ kind: 'overview' });
      setChoice(null);
      goTo('recognize');
      return;
    }
    // A question the recognition asked itself: the next one (the date, then the amount, then the order of
    // an ambiguous date), the rows, or — the field it just asked still empty — the end
    const step = afterStartApply(next, session.inferred, confirmed, view.field);
    if (step.stage === 'unreadable') return onUnreadable();
    goTo(show(step, next, session, true));
  };

  const anchor = accounts.find((account) => account.id === session?.anchorId);

  return {
    csv: session?.csv ?? null,
    inferred: session?.inferred ?? null,
    mapping,
    /** The mapping the open field's checked option would give (the committed one elsewhere). */
    pending: shown,
    /**
     * The balance proved the amount's signs reversed and the import cannot flip them: the mapping
     * cannot be accepted until another amount column is chosen.
     */
    signsBlocked:
      session !== null && mapping !== null && signsBlockImport(mapping, session.inferred),
    view,
    options,
    choice,
    canApply: chosen !== null,
    /** The rows of the file as they would be read: the committed mapping, or the open field's choice. */
    rows: preview?.rows.slice(0, SAMPLE_ROWS) ?? [],
    /** The mapping cannot be previewed yet (no date or amount): the sample row waits for a choice. */
    previewable: config !== null,
    previewLoading: isLoadingFirst,
    previewUpdating: isUpdating,
    previewError,
    /** The currency of rows whose file has no currency column; the anchor account's. */
    currency: anchor?.currency ?? '',
    begin,
    reset,
    openFields,
    openField,
    back,
    select: setChoice,
    apply,
  };
}

export type Recognition = ReturnType<typeof useRecognition>;
