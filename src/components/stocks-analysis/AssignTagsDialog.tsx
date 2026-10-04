import { useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { StockInvestmentWithTags, StockTag } from '@shared/schema';
import { stockTagsApi } from '@/lib/tauri-api';
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
  /** Position(s) preselected when the dialog opens. */
  initialStockIds: string[];
}

/**
 * "Přiřadit štítky" (prototype `stocks-analysis.html`): pick positions and
 * tags. With one position the chips start from its current tags and the
 * save sets them exactly (so a tag can also be removed); with several
 * positions the chosen tags are added to each of them.
 */
export function AssignTagsDialog(props: AssignTagsDialogProps) {
  const { t } = useTranslation('reports');
  return (
    <Dialog open={props.open} onOpenChange={props.onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('stocksAnalysis.assign.title')}</DialogTitle>
          <DialogDescription>{t('stocksAnalysis.assign.description')}</DialogDescription>
        </DialogHeader>
        {/* Mounted per opening so the selection starts from the preselected position */}
        {props.open && <AssignForm {...props} />}
      </DialogContent>
    </Dialog>
  );
}

function AssignForm({ onOpenChange, stocks, tags, initialStockIds }: AssignTagsDialogProps) {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const [stockIds, setStockIds] = useState<string[]>(initialStockIds);
  const [tagIds, setTagIds] = useState<string[]>(() =>
    initialStockIds.length === 1
      ? (stocks.find((s) => s.id === initialStockIds[0])?.tags.map((x) => x.id) ?? [])
      : []
  );
  const single = stockIds.length === 1;

  const mutation = useMutation({
    mutationFn: async () => {
      for (const id of stockIds) {
        const stock = stocks.find((s) => s.id === id);
        if (!stock) continue;
        const next = single ? tagIds : [...new Set([...stock.tags.map((x) => x.id), ...tagIds])];
        await stockTagsApi.setForInvestment(id, next);
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['stocks-analysis'] });
      queryClient.invalidateQueries({ queryKey: ['tag-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['stock-twr'] });
      toast.success(t('stocksAnalysis.assign.saved'));
      onOpenChange(false);
    },
    onError: (error: Error) => toast.error(tc('status.error'), { description: error.message }),
  });

  const toggle = (list: string[], id: string) =>
    list.includes(id) ? list.filter((x) => x !== id) : [...list, id];

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
      <div className="grid max-h-[220px] gap-2 overflow-y-auto pr-1">
        {stocks.map((s) => (
          <label
            key={s.id}
            className="flex cursor-pointer items-center gap-[10px] text-table text-ink"
          >
            <Checkbox
              checked={stockIds.includes(s.id)}
              onCheckedChange={() => setStockIds((prev) => toggle(prev, s.id))}
            />
            <span className="font-600">{s.companyName}</span>
            <span className="text-micro font-500 text-ink-4">{s.ticker}</span>
          </label>
        ))}
      </div>

      <Label className="mb-2 mt-5">{t('stocksAnalysis.assign.tags')}</Label>
      {tags.length === 0 ? (
        <p className="text-table text-ink-3">{t('stocksAnalysis.assign.noTags')}</p>
      ) : (
        <div className="flex flex-wrap gap-1.5">
          {tags.map((tag) => (
            <Chip
              key={tag.id}
              active={tagIds.includes(tag.id)}
              onClick={() => setTagIds((prev) => toggle(prev, tag.id))}
            >
              {tag.name}
            </Chip>
          ))}
        </div>
      )}
      <p className="mt-3 text-micro font-500 text-ink-4">
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
