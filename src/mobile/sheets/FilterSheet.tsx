import { useId, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Layers, Tag } from 'lucide-react';
import type { TransactionCategory } from '@shared/schema';
import { useCategoryName } from '@/hooks/use-categories';
import { CategoryIconChip } from '@/components/common/CategoryIcon';
import { Button } from '@/components/ui/button';
import { Sheet } from '@/mobile/components/Sheet';
import { DEFAULT_LEDGER, type LedgerState, type PeriodPreset } from '@/mobile/lib/ledger';

interface FilterSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The ledger's state; the sheet edits a copy and hands it back on Použít. */
  state: LedgerState;
  categories: readonly TransactionCategory[];
  onApply: (state: LedgerState) => void;
}

const PRESETS: readonly PeriodPreset[] = [
  'thisMonth',
  'lastMonth',
  'thisYear',
  'allTime',
  'custom',
];

/**
 * The ledger's filter sheet (spec §8.4, prototype note 8): the category — every one, the
 * uncategorized, or one category — and the period, from this month to a custom range with two
 * date fields. It edits a copy: Použít applies it, Zrušit filtry goes back to every movement of
 * every period (the segment and the search stay). Picking a period lets go of a month tapped in
 * the flows strip.
 */
export function FilterSheet({ open, onOpenChange, state, categories, onApply }: FilterSheetProps) {
  const { t } = useTranslation('mobile');
  const categoryName = useCategoryName();
  const [draft, setDraft] = useState(state);
  // Every opening starts from the ledger's current filter
  const [openedFor, setOpenedFor] = useState(false);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) setDraft(state);
  }

  const apply = (next: LedgerState) => {
    onApply(next);
    onOpenChange(false);
  };
  const setCategory = (categoryId: string | null) => setDraft((d) => ({ ...d, categoryId }));
  const setPreset = (preset: PeriodPreset) => setDraft((d) => ({ ...d, preset, month: null }));
  const setDate = (end: 'from' | 'to', value: string) =>
    setDraft((d) => ({ ...d, preset: 'custom', month: null, [end]: value || null }));
  // A month from the flows strip is a custom range of that month
  const preset = draft.month !== null ? 'custom' : draft.preset;

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('account.filter.title')}
      full
      footer={
        <>
          <Button
            variant="ghost"
            className="h-12 rounded-[10px] px-[18px] text-m-row"
            onClick={() => apply({ ...DEFAULT_LEDGER, kind: state.kind, search: state.search })}
          >
            {t('account.filter.reset')}
          </Button>
          <Button className="h-12 flex-1 rounded-[10px] text-m-row" onClick={() => apply(draft)}>
            {t('account.filter.apply')}
          </Button>
        </>
      }
    >
      <PickGroup title={t('account.filter.category')}>
        <PickRow
          icon={<NeutralChip icon={<Layers />} />}
          label={t('account.filter.all')}
          checked={draft.categoryId === null}
          onSelect={() => setCategory(null)}
        />
        <PickRow
          icon={<NeutralChip icon={<Tag />} />}
          label={t('bank_accounts:filters.uncategorized')}
          checked={draft.categoryId === 'uncategorized'}
          onSelect={() => setCategory('uncategorized')}
        />
        {categories.map((category) => (
          <PickRow
            key={category.id}
            icon={<CategoryIconChip category={category} />}
            label={categoryName(category)}
            checked={draft.categoryId === category.id}
            onSelect={() => setCategory(category.id)}
          />
        ))}
      </PickGroup>
      <PickGroup title={t('account.filter.period')}>
        {PRESETS.map((p) => (
          <PickRow
            key={p}
            label={t(`bank_accounts:filters.${p}`)}
            checked={preset === p}
            onSelect={() => setPreset(p)}
          />
        ))}
      </PickGroup>
      {preset === 'custom' && (
        <div className="mb-2 grid grid-cols-2 gap-2.5">
          <DateField
            label={t('account.filter.from')}
            value={draft.from ?? ''}
            onChange={(value) => setDate('from', value)}
          />
          <DateField
            label={t('account.filter.to')}
            value={draft.to ?? ''}
            onChange={(value) => setDate('to', value)}
          />
        </div>
      )}
    </Sheet>
  );
}

/** A titled radio list of the sheet. */
function PickGroup({ title, children }: { title: string; children: ReactNode }) {
  const id = useId();
  return (
    <section className="mb-2">
      <h3 id={id} className="mx-1 mb-0.5 mt-2 text-m-sub font-semibold text-ink-3">
        {title}
      </h3>
      <div role="radiogroup" aria-labelledby={id} className="divide-y divide-line-soft">
        {children}
      </div>
    </section>
  );
}

/** A 44 pt radio row (`.m-pick`): an optional chip, the label, a check on the chosen one. */
function PickRow({
  icon,
  label,
  checked,
  onSelect,
}: {
  icon?: ReactNode;
  label: string;
  checked: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      onClick={onSelect}
      className="flex min-h-11 w-full items-center gap-3 px-1 py-1.5 text-left text-m-body text-ink [-webkit-touch-callout:none] active:bg-well focus-visible:outline-none focus-visible:shadow-focus"
    >
      {icon}
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {checked && <Check aria-hidden className="size-[18px] flex-none text-ink" strokeWidth={2} />}
    </button>
  );
}

/** The 28 pt chip of the rows that are not a category (all, uncategorized). */
function NeutralChip({ icon }: { icon: ReactNode }) {
  return (
    <span
      aria-hidden
      className="grid size-7 shrink-0 place-items-center rounded-full bg-well-2 text-ink-3 [&_svg]:size-[15px]"
    >
      {icon}
    </span>
  );
}

/** A 48 pt date field (`.m-field__i`) with its label above. */
function DateField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const id = useId();
  return (
    <div className="min-w-0">
      <label htmlFor={id} className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2">
        {label}
      </label>
      <input
        id={id}
        type="date"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="block h-12 w-full min-w-0 rounded-[10px] border border-line-strong bg-paper px-3 text-m-body text-ink num transition-[border-color,box-shadow] duration-fast focus:border-focus focus:shadow-focus focus-visible:outline-none"
      />
    </div>
  );
}
