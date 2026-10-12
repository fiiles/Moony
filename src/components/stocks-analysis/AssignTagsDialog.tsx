import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Search } from 'lucide-react';
import { toast } from 'sonner';
import type { StockInvestmentWithTags, StockTag, StockTagGroup } from '@shared/schema';
import { stockTagsApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { cn } from '@/lib/utils';
import { matchesSearch } from '@/utils/holdings-search';
import {
  changedPositions,
  initialActiveId,
  pickGroupTag,
  toggleFreeTag,
} from '@/utils/stock-analysis';
import { AssetLogo } from '@/components/common/AssetLogo';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input, InputWrap } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Button } from '@/components/ui/button';
import { Chip } from '@/components/ui/chip';

interface AssignTagsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  stocks: StockInvestmentWithTags[];
  tags: StockTag[];
  groups: StockTagGroup[];
  /** The position the dialog opens on is the first of these; none opens on an untagged one. */
  initialStockIds: string[];
}

/**
 * "Přiřadit štítky": click a position, pick its tags. The chips always edit the active position
 * only; every position keeps its own draft while the dialog is open, and one save writes all the
 * changed ones. A tag group is a single choice ("Bez štítku" or one of its tags), tags without a
 * group combine freely.
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
        {/* Mounted per opening so the drafts and the active position start over */}
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

  const [activeId, setActiveId] = useState(() => initialActiveId(stocks, initialStockIds));
  // Tag ids per edited position: a draft stays while the dialog is open, whatever is active
  const [drafts, setDrafts] = useState<ReadonlyMap<string, readonly string[]>>(() => new Map());
  const [query, setQuery] = useState('');

  const active = stocks.find((s) => s.id === activeId);
  const tagIdsOf = (stock: StockInvestmentWithTags): readonly string[] =>
    drafts.get(stock.id) ?? stock.tags.map((tag) => tag.id);
  const changed = useMemo(() => changedPositions(stocks, drafts), [stocks, drafts]);
  const visible = useMemo(
    () => stocks.filter((s) => matchesSearch(query, [s.companyName, s.ticker])),
    [stocks, query]
  );

  /** The tags a position will have after saving, as the list shows them. */
  const tagNamesOf = (stock: StockInvestmentWithTags): string[] =>
    tagIdsOf(stock)
      .flatMap((id) => {
        const tag = tags.find((x) => x.id === id) ?? stock.tags.find((x) => x.id === id);
        return tag ? [tag.name] : [];
      })
      .sort((a, b) => a.localeCompare(b));

  const edit = (change: (tagIds: readonly string[]) => string[]) => {
    if (!active) return;
    const base = tagIdsOf(active);
    setDrafts((prev) => new Map(prev).set(active.id, change(prev.get(active.id) ?? base)));
  };

  const listRef = useRef<HTMLUListElement>(null);
  // Open with the active position in view (the list scrolls, a long portfolio hides it)
  useEffect(() => {
    const list = listRef.current;
    const row = list?.querySelector<HTMLElement>('[aria-current="true"]');
    if (list && row) {
      list.scrollTop = Math.max(0, row.offsetTop - (list.clientHeight - row.offsetHeight) / 2);
    }
    // Once, on opening: later clicks and arrow keys keep the list where the user put it
  }, []);

  // Up and Down move the active position; the row that was reached takes the focus
  const onListKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    const at = visible.findIndex((s) => s.id === activeId);
    const next =
      event.key === 'ArrowDown'
        ? visible[Math.min(at + 1, visible.length - 1)]
        : visible[Math.max(at - 1, 0)];
    if (!next) return;
    event.preventDefault();
    setActiveId(next.id);
    listRef.current
      ?.querySelector<HTMLElement>(`[data-position-id="${CSS.escape(next.id)}"]`)
      ?.focus();
  };
  // One row of the list is in the tab order: the active one, else the first that is shown
  const tabStopId = visible.some((s) => s.id === activeId) ? activeId : visible[0]?.id;

  const mutation = useMutation({
    mutationFn: async () => {
      for (const id of changed) {
        await stockTagsApi.setForInvestment(id, [...(drafts.get(id) ?? [])]);
      }
    },
    // Also after a failure: positions saved before it keep their new tags (and, once the report
    // is fetched again, show no change any more)
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

  const activeTagIds = active ? tagIdsOf(active) : [];

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (changed.length > 0) mutation.mutate();
      }}
    >
      <Label className="mb-2">{t('stocksAnalysis.assign.positions')}</Label>
      <InputWrap icon={<Search />} className="mb-2">
        <Input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          // Enter belongs to the list here, not to the form's save
          onKeyDown={(e) => {
            if (e.key === 'Enter') e.preventDefault();
          }}
          placeholder={t('stocksAnalysis.assign.search')}
          aria-label={t('stocksAnalysis.assign.search')}
        />
      </InputWrap>
      <ul
        ref={listRef}
        aria-label={t('stocksAnalysis.assign.positions')}
        onKeyDown={onListKeyDown}
        className="relative m-0 max-h-[196px] list-none overflow-y-auto rounded-r2 border border-line p-0"
      >
        {visible.map((s) => {
          const names = tagNamesOf(s);
          const isActive = s.id === activeId;
          return (
            <li key={s.id} className="border-t border-line-soft first:border-t-0">
              <button
                type="button"
                data-position-id={s.id}
                aria-current={isActive ? 'true' : undefined}
                tabIndex={s.id === tabStopId ? 0 : -1}
                onClick={() => setActiveId(s.id)}
                className={cn(
                  'relative grid w-full grid-cols-[30px_1fr_auto] items-center gap-[10px] px-3 py-[7px] text-left transition-colors duration-fast hover:bg-well/60 focus-visible:z-10 focus-visible:outline-none focus-visible:shadow-focus',
                  isActive &&
                    'bg-well before:absolute before:inset-y-0 before:left-0 before:w-[2px] before:bg-dark'
                )}
              >
                <AssetLogo ticker={s.ticker} type="stock" />
                <span className="min-w-0">
                  <b className="block truncate text-table font-650 text-ink">{s.companyName}</b>
                  <small
                    className={cn(
                      'mt-[2px] block truncate text-micro font-500',
                      names.length > 0 ? 'text-ink-3' : 'text-ink-4'
                    )}
                  >
                    {names.length > 0 ? names.join(' · ') : t('stocksAnalysis.holdings.noTag')}
                  </small>
                </span>
                {changed.includes(s.id) && (
                  <span className="inline-flex items-center gap-1.5 text-micro font-600 text-ink-2">
                    <span aria-hidden className="size-1.5 rounded-full bg-dark" />
                    {t('stocksAnalysis.assign.changed')}
                  </span>
                )}
              </button>
            </li>
          );
        })}
      </ul>

      {active && (
        <Label className="mb-1 mt-5">
          {t('stocksAnalysis.assign.forPosition', {
            name: `${active.companyName} · ${active.ticker}`,
          })}
        </Label>
      )}
      {rows.length === 0 && freeTags.length === 0 ? (
        <p className="py-2 text-table text-ink-3">{t('stocksAnalysis.assign.noTags')}</p>
      ) : (
        active && (
          <div>
            {rows.map(({ group, tags: groupTags }) => {
              const chosen = groupTags.find((tag) => activeTagIds.includes(tag.id));
              return (
                <ChipRow key={group.id} label={group.name} hint={t('stocksAnalysis.assign.oneTag')}>
                  <Chip
                    active={chosen === undefined}
                    className={quiet(chosen === undefined)}
                    onClick={() => edit((ids) => pickGroupTag(ids, tags, group.id, null))}
                  >
                    {t('stocksAnalysis.assign.none')}
                  </Chip>
                  {groupTags.map((tag) => (
                    <Chip
                      key={tag.id}
                      active={chosen?.id === tag.id}
                      onClick={() => edit((ids) => pickGroupTag(ids, tags, group.id, tag.id))}
                    >
                      {tag.name}
                    </Chip>
                  ))}
                </ChipRow>
              );
            })}
            {freeTags.length > 0 && (
              <ChipRow
                label={t('stocksAnalysis.assign.noGroup')}
                hint={t('stocksAnalysis.assign.manyTags')}
              >
                {freeTags.map((tag) => (
                  <Chip
                    key={tag.id}
                    active={activeTagIds.includes(tag.id)}
                    onClick={() => edit((ids) => toggleFreeTag(ids, tags, tag.id))}
                  >
                    {tag.name}
                  </Chip>
                ))}
              </ChipRow>
            )}
          </div>
        )
      )}
      <p className="mt-2 text-micro font-500 text-ink-4">{t('stocksAnalysis.assign.hint')}</p>

      <DialogFooter className="mt-5">
        <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
          {tc('buttons.cancel')}
        </Button>
        {changed.length > 0 && (
          <span className="text-micro font-500 text-ink-4">
            {t('stocksAnalysis.assign.changedCount', { count: changed.length })}
          </span>
        )}
        <Button type="submit" disabled={changed.length === 0} loading={mutation.isPending}>
          {t('stocksAnalysis.assign.save')}
        </Button>
      </DialogFooter>
    </form>
  );
}

/** "Bez štítku" is not a tag: dashed and muted until chosen. */
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
