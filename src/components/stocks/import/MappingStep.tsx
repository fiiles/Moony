import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { FileText, TriangleAlert } from 'lucide-react';
import { Alert } from '@/components/ui/alert';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { fileNameFromPath } from '@/components/bank-accounts/csv-import/import-config';
import type {
  SavedStockImportFormat,
  StockCsvInspection,
  StockTypeValueAction,
} from '@shared/schema';
import { MappingForm, type RememberFormat } from './MappingForm';
import { RawDataPreview } from './RawDataPreview';
import {
  columnOptions,
  groupTypeValues,
  isSourceId,
  mappedColumns,
  sourceMismatch,
  summarizeMapping,
  type MappingState,
  type MappingSummaryItem,
} from './import-config';
import { useSourceLabel } from './use-source-label';

/** Values shown per meaning in the summary before "+N". */
const TYPE_VALUES_SHOWN = 4;

interface MappingStepProps {
  filePath: string;
  inspection: StockCsvInspection;
  mapping: MappingState;
  onMappingChange: (patch: Partial<MappingState>) => void;
  /** Data rows skipped after the header (the file is read again when it changes). */
  skipRows: number;
  /** Read the file again with another header line (0-based) or number of skipped rows. */
  onLayoutChange: (layout: { headerRow: number; skipRows: number }) => void;
  /** Changes with every re-read, so the layout fields start from the new values. */
  inspectRevision: number;
  /** The file is being read again (layout or source changed). */
  isInspecting: boolean;
  /** The source the user picked on step 1, if they picked one. */
  chosenSource: string | null;
  formats: readonly SavedStockImportFormat[];
  /** The full form is open (otherwise a known source shows its summary). */
  editing: boolean;
  onEditingChange: (editing: boolean) => void;
  /** Back to what the detection found. */
  onResetMapping: () => void;
  remember: RememberFormat | null;
  onRememberChange: (patch: Partial<RememberFormat>) => void;
}

/**
 * Step 2 (spec §3): the file bar, then either a compact summary of what a known
 * source maps — most users go straight on — or the full mapping form (other
 * brokers, saved formats, edited presets), and the first rows of the file with
 * the mapped columns marked.
 */
export function MappingStep({
  filePath,
  inspection,
  mapping,
  onMappingChange,
  skipRows,
  onLayoutChange,
  inspectRevision,
  isInspecting,
  chosenSource,
  formats,
  editing,
  onEditingChange,
  onResetMapping,
  remember,
  onRememberChange,
}: MappingStepProps) {
  const { t } = useTranslation('stocks');
  const sourceLabel = useSourceLabel(formats);

  const options = useMemo(
    () =>
      columnOptions(inspection.headers, inspection.sampleRows, {
        blank: (position) => t('importWizard.mapping.blankColumn', { position }),
        blankAfter: (position, after) =>
          t('importWizard.mapping.blankColumnAfter', { position, after }),
      }),
    [inspection.headers, inspection.sampleRows, t]
  );
  const mapped = useMemo(() => mappedColumns(mapping), [mapping]);

  const detected = inspection.detectedSource;
  const mismatch = sourceMismatch(chosenSource, detected);
  const summarize = !editing && inspection.config != null && detected != null;

  return (
    <div>
      <div className="mb-4 flex items-center gap-3 rounded-r2 border border-line bg-well px-3 py-2.5 text-table text-ink-2">
        <FileText className="size-4 shrink-0 text-ink-3" aria-hidden />
        <span className="min-w-0 truncate">
          <b className="font-650 text-ink">{fileNameFromPath(filePath)}</b> ·{' '}
          {t('importWizard.filebar.rows', { count: inspection.rowCount })}
          {detected && (
            <>
              {' '}
              · {sourceLabel(detected)}{' '}
              <Badge variant="gain" className="ml-1.5 align-[1px]">
                {t('importWizard.filebar.detected')}
              </Badge>
            </>
          )}
        </span>
        <span className="ml-auto shrink-0 text-micro font-500 text-ink-4">
          {t('importWizard.filebar.format', {
            delimiter:
              inspection.delimiter === '\t' ? t('importWizard.filebar.tab') : inspection.delimiter,
            encoding: inspection.encoding,
          })}
        </span>
      </div>

      {mismatch !== 'none' && chosenSource && (
        <Alert className="mb-4">
          <TriangleAlert aria-hidden />
          <p className="m-0">
            {mismatch === 'other' && detected
              ? t('importWizard.mismatch.other', {
                  chosen: sourceLabel(chosenSource),
                  detected: sourceLabel(detected),
                })
              : t('importWizard.mismatch.unknown', { chosen: sourceLabel(chosenSource) })}
          </p>
        </Alert>
      )}

      {summarize ? (
        <MappingSummary
          source={sourceLabel(mapping.source)}
          items={summarizeMapping(mapping, options)}
          groups={groupTypeValues(mapping, inspection)}
          typeColumn={options.find((option) => option.index === mapping.typeColumn)?.label}
          sourceKey={mapping.source}
          onEdit={() => onEditingChange(true)}
        />
      ) : (
        <>
          {inspection.config != null && detected != null && (
            <div className="mb-3 flex justify-end">
              <Button type="button" variant="ghost" size="sm" onClick={onResetMapping}>
                {t('importWizard.mapping.backToPreset', { source: sourceLabel(detected) })}
              </Button>
            </div>
          )}
          <MappingForm
            inspection={inspection}
            mapping={mapping}
            options={options}
            onChange={onMappingChange}
            skipRows={skipRows}
            onLayoutChange={onLayoutChange}
            inspectRevision={inspectRevision}
            isInspecting={isInspecting}
            remember={remember}
            onRememberChange={onRememberChange}
          />
        </>
      )}

      <div className="mt-5">
        <RawDataPreview inspection={inspection} options={options} mapped={mapped} />
      </div>
    </div>
  );
}

interface MappingSummaryProps {
  /** Display name of the source. */
  source: string;
  items: readonly MappingSummaryItem[];
  groups: Record<StockTypeValueAction, string[]>;
  typeColumn: string | undefined;
  /** The source id, to look the note up. */
  sourceKey: string;
  onEdit: () => void;
}

const GROUP_BADGES: Record<StockTypeValueAction, 'dark' | 'outline' | 'default'> = {
  buy: 'dark',
  sell: 'outline',
  skip: 'default',
};

/** What a known source maps: columns and the meaning of the type values, with "Upravit mapování". */
function MappingSummary({
  source,
  items,
  groups,
  typeColumn,
  sourceKey,
  onEdit,
}: MappingSummaryProps) {
  const { t } = useTranslation('stocks');
  // Only the built-in sources have a note (a saved format's id would read as a namespace).
  const note = isSourceId(sourceKey)
    ? t(`importWizard.sources.${sourceKey}.note`, { defaultValue: '' })
    : '';
  const actions: StockTypeValueAction[] = ['buy', 'sell', 'skip'];
  // Only worth showing when some value is read as a trade; otherwise the review shows the truth.
  const hasValues = groups.buy.length + groups.sell.length > 0;

  const valueText = (item: MappingSummaryItem) => {
    switch (item.kind) {
      case 'sign':
        return t('importWizard.mapping.summary.sign');
      case 'instrument':
        return t('importWizard.mapping.summary.instrument');
      case 'comment':
        return t('importWizard.mapping.summary.comment', { column: item.value });
      default:
        return item.value;
    }
  };

  return (
    <div>
      <div className="rounded-r3 border border-line bg-well px-4 py-3.5">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <h4 className="m-0 text-caption font-700 text-ink-2">
              {t('importWizard.mapping.summary.title', { source })}
            </h4>
            <p className="mb-0 mt-0.5 text-micro font-500 text-ink-4">
              {t('importWizard.mapping.summary.hint')}
            </p>
          </div>
          <Button type="button" variant="outline" size="sm" onClick={onEdit}>
            {t('importWizard.mapping.edit')}
          </Button>
        </div>

        <dl className="m-0 mt-3.5 grid grid-cols-3 gap-x-5 gap-y-2.5">
          {items.map((item) => (
            <div key={item.role} className="flex min-w-0 items-baseline gap-1.5 text-table">
              <dt className="shrink-0 text-ink-4">{t(`importWizard.roles.${item.role}`)}</dt>
              <dd className="m-0 flex min-w-0 items-baseline gap-1.5">
                <span className="text-ink-5" aria-hidden>
                  ←
                </span>
                <b className="truncate font-650 text-ink" title={valueText(item)}>
                  {valueText(item)}
                </b>
              </dd>
            </div>
          ))}
        </dl>

        {hasValues && (
          <div className="mt-3.5 border-t border-line pt-3">
            <p className="mb-2 mt-0 text-micro font-600 text-ink-4">
              {t('importWizard.mapping.summary.values', { column: typeColumn ?? '' })}
            </p>
            <ul className="m-0 grid list-none gap-1.5 p-0">
              {actions
                .filter((action) => groups[action].length > 0)
                .map((action) => {
                  const values = groups[action];
                  const shown = values.slice(0, TYPE_VALUES_SHOWN);
                  const more = values.length - shown.length;
                  return (
                    <li key={action} className="flex items-baseline gap-2.5 text-table">
                      <Badge variant={GROUP_BADGES[action]} className="shrink-0">
                        {t(`importWizard.mapping.action.${action}`)}
                      </Badge>
                      <span className="min-w-0 truncate text-ink-2" title={values.join(', ')}>
                        {shown.join(', ')}
                        {more > 0 && (
                          <span className="text-ink-4">
                            {' '}
                            {t('importWizard.mapping.summary.more', { count: more })}
                          </span>
                        )}
                      </span>
                    </li>
                  );
                })}
            </ul>
          </div>
        )}
      </div>
      {note && <p className="mb-0 mt-3 text-caption text-ink-3">{note}</p>}
    </div>
  );
}
