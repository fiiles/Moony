import { useId, useMemo, useState, type ReactNode } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { StockInvestmentWithTags, StockTag, StockTagGroup } from '@shared/schema';
import { stockTagsApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { cn } from '@/lib/utils';
import { PICK_KEEP, PICK_NONE, nextTagIds } from '@/utils/stock-analysis';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Checkbox } from '@/components/ui/checkbox';
import { Label, LabelHint } from '@/components/ui/label';
import { Button } from '@/components/ui/button';
import { Chip } from '@/components/ui/chip';

interface AssignTagsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  stocks: StockInvestmentWithTags[];
  tags: StockTag[];
  groups: StockTagGroup[];
  /** Position(s) preselected when the dialog opens. */
  initialStockIds: string[];
}

/** What the chips show: per group the choice, plus the checked tags without a group. */
interface Selection {
  stockIds: string[];
  /** Per tag group: `PICK_KEEP`, `PICK_NONE` or the id of the chosen tag. */
  picks: Record<string, string>;
  /** Ids of the checked tags without a group. */
  free: string[];
}

/**
 * The starting choices for these positions. One position starts from its own tags (a group it
 * has no tag in starts on "Bez štítku"); several start with every group on "Beze změny" and
 * nothing checked, so saving without touching a chip changes nothing.
 */
function selectionFor(
  stockIds: string[],
  stocks: readonly StockInvestmentWithTags[],
  groups: readonly StockTagGroup[]
): Selection {
  const only = stockIds.length === 1 ? stocks.find((s) => s.id === stockIds[0]) : undefined;
  const picks: Record<string, string> = {};
  for (const group of groups) {
    picks[group.id] = only
      ? (only.tags.find((tag) => tag.groupId === group.id)?.id ?? PICK_NONE)
      : PICK_KEEP;
  }
  const free = only ? only.tags.filter((tag) => tag.groupId === null).map((tag) => tag.id) : [];
  return { stockIds, picks, free };
}

/**
 * "Přiřadit štítky" (spec `docs/specs/2026-10-11-stocks-analysis-redesign-design.md`): pick
 * positions and, per tag group, one tag. A group is a choice — the chosen tag replaces the
 * position's current one, "Bez štítku" removes it, and for several positions "Beze změny" leaves
 * it. Tags without a group can be combined freely.
 */
export function AssignTagsDialog(props: AssignTagsDialogProps) {
  const { t } = useTranslation('reports');
  return (
    <Dialog open={props.open} onOpenChange={props.onOpenChange}>
      <DialogContent className="max-w-[640px]">
        <DialogHeader>
          <DialogTitle>{t('stocksAnalysis.assign.title')}</DialogTitle>
          <DialogDescription>{t('stocksAnalysis.assign.description')}</DialogDescription>
        </DialogHeader>
        {/* Mounted per opening so the choices start from the preselected position */}
        {props.open && <AssignForm {...props} />}
      </DialogContent>
    </Dialog>
  );
}

function AssignForm({
  onOpenChange,
  stocks,
  tags,
  groups,
  initialStockIds,
}: AssignTagsDialogProps) {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();

  // A group without tags offers nothing to choose, so it gets no row
  const rows = useMemo(
    () =>
      [...groups]
        .sort((a, b) => a.createdAt - b.createdAt)
        .map((group) => ({
          group,
          tags: tags
            .filter((tag) => tag.groupId === group.id)
            .sort((a, b) => a.name.localeCompare(b.name)),
        }))
        .filter((row) => row.tags.length > 0),
    [groups, tags]
  );
  const freeTags = useMemo(
    () => tags.filter((tag) => tag.groupId === null).sort((a, b) => a.name.localeCompare(b.name)),
    [tags]
  );

  const [selection, setSelection] = useState<Selection>(() =>
    selectionFor(initialStockIds, stocks, groups)
  );
  const { stockIds, picks, free } = selection;
  const single = stockIds.length === 1;

  // The positions the dialog opened for come first; their order then stays put while ticking
  const [firstIds] = useState(initialStockIds);
  const ordered = useMemo(
    () => [
      ...firstIds.flatMap((id) => stocks.filter((s) => s.id === id)),
      ...stocks.filter((s) => !firstIds.includes(s.id)),
    ],
    [stocks, firstIds]
  );

  const mutation = useMutation({
    mutationFn: async () => {
      for (const id of stockIds) {
        const stock = stocks.find((s) => s.id === id);
        if (!stock) continue;
        await stockTagsApi.setForInvestment(id, nextTagIds(stock.tags, picks, free, single, tags));
      }
    },
    // Also after a failure: positions saved before it keep their new tags
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['stocks-analysis'] });
      queryClient.invalidateQueries({ queryKey: ['stock-performance'] });
    },
    onSuccess: () => {
      toast.success(t('stocksAnalysis.assign.saved'));
      onOpenChange(false);
    },
    onError: (error: Error) =>
      toast.error(tc('status.error'), { description: translateApiError(error, tc) }),
  });

  // A different set of positions starts the choices over (what "unchanged" means depends on it)
  const togglePosition = (id: string) =>
    setSelection(
      selectionFor(
        stockIds.includes(id) ? stockIds.filter((x) => x !== id) : [...stockIds, id],
        stocks,
        groups
      )
    );
  const pick = (groupId: string, value: string) =>
    setSelection((prev) => ({ ...prev, picks: { ...prev.picks, [groupId]: value } }));
  const toggleFree = (id: string) =>
    setSelection((prev) => ({
      ...prev,
      free: prev.free.includes(id) ? prev.free.filter((x) => x !== id) : [...prev.free, id],
    }));

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (stockIds.length > 0) mutation.mutate();
      }}
    >
      <Label className="mb-2">
        {t('stocksAnalysis.assign.positions')}
        <LabelHint>{t('stocksAnalysis.assign.selected', { count: stockIds.length })}</LabelHint>
      </Label>
      <div className="max-h-[196px] overflow-y-auto rounded-r2 border border-line">
        {ordered.map((s) => (
          <label
            key={s.id}
            className="grid cursor-pointer grid-cols-[16px_1fr_auto] items-center gap-[10px] border-t border-line-soft px-3 py-[7px] text-table text-ink transition-colors duration-fast first:border-t-0 hover:bg-well"
          >
            <Checkbox
              checked={stockIds.includes(s.id)}
              onCheckedChange={() => togglePosition(s.id)}
            />
            <span className="min-w-0 truncate">
              <span className="font-650">{s.companyName}</span>{' '}
              <span className="text-micro font-500 text-ink-4">{s.ticker}</span>
            </span>
            <span className="text-right text-micro font-500 text-ink-4">
              {s.tags.length > 0
                ? s.tags.map((x) => x.name).join(' · ')
                : t('stocksAnalysis.assign.none')}
            </span>
          </label>
        ))}
      </div>

      <Label className="mb-1 mt-5">{t('stocksAnalysis.assign.tags')}</Label>
      {rows.length === 0 && freeTags.length === 0 ? (
        <p className="py-2 text-table text-ink-3">{t('stocksAnalysis.assign.noTags')}</p>
      ) : (
        <div>
          {rows.map(({ group, tags: groupTags }) => (
            <ChipRow key={group.id} label={group.name} hint={t('stocksAnalysis.assign.oneTag')}>
              {!single && (
                <Chip
                  active={picks[group.id] === PICK_KEEP}
                  className={quiet(picks[group.id] === PICK_KEEP)}
                  onClick={() => pick(group.id, PICK_KEEP)}
                >
                  {t('stocksAnalysis.assign.keep')}
                </Chip>
              )}
              {groupTags.map((tag) => (
                <Chip
                  key={tag.id}
                  active={picks[group.id] === tag.id}
                  onClick={() => pick(group.id, tag.id)}
                >
                  {tag.name}
                </Chip>
              ))}
              <Chip
                active={picks[group.id] === PICK_NONE}
                className={quiet(picks[group.id] === PICK_NONE)}
                onClick={() => pick(group.id, PICK_NONE)}
              >
                {t('stocksAnalysis.assign.none')}
              </Chip>
            </ChipRow>
          ))}
          {freeTags.length > 0 && (
            <ChipRow
              label={t('stocksAnalysis.assign.noGroup')}
              hint={t('stocksAnalysis.assign.manyTags')}
            >
              {freeTags.map((tag) => (
                <Chip
                  key={tag.id}
                  active={free.includes(tag.id)}
                  onClick={() => toggleFree(tag.id)}
                >
                  {tag.name}
                </Chip>
              ))}
            </ChipRow>
          )}
        </div>
      )}
      <p className="mt-2 text-micro font-500 text-ink-4">
        {single ? t('stocksAnalysis.assign.hintSingle') : t('stocksAnalysis.assign.hintMany')}
      </p>

      <DialogFooter className="mt-5">
        <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
          {tc('buttons.cancel')}
        </Button>
        <Button type="submit" disabled={stockIds.length === 0} loading={mutation.isPending}>
          {t('stocksAnalysis.assign.save')}
        </Button>
      </DialogFooter>
    </form>
  );
}

/** "Beze změny" and "Bez štítku" are not tags: dashed and muted until chosen. */
const quiet = (active: boolean) => cn(!active && 'border-dashed text-ink-3');

/** One tag group: its name and rule on the left, the chips on the right. */
function ChipRow({ label, hint, children }: { label: string; hint: string; children: ReactNode }) {
  const id = useId();
  return (
    <div className="grid grid-cols-[132px_1fr] items-start gap-[10px] border-t border-line-soft py-[11px] first:border-t-0">
      <div id={id}>
        <b className="block text-caption font-700 text-ink-2">{label}</b>
        <small className="text-micro font-500 text-ink-4">{hint}</small>
      </div>
      <div role="group" aria-labelledby={id} className="flex flex-wrap gap-1.5">
        {children}
      </div>
    </div>
  );
}
