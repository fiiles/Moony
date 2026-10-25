import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useTranslation } from 'react-i18next';
import { insertStockSplitSchema, type StockSplit } from '@shared/schema';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { useFormat } from '@/lib/use-format';
import { useStockSplitMutations } from '@/hooks/use-stock-split-mutations';
import { isoDayToUnix, ratioText } from '@/utils/stock-splits';

/** Today as the yyyy-mm-dd of its UTC day: the backend refuses a split day after it (D11). */
function todayIso(): string {
  return new Date().toISOString().split('T')[0];
}

const ratioPart = insertStockSplitSchema.shape.numerator.refine(
  (value) => Number(value) > 0,
  'validation.stockSplitRatioInvalid'
);

const formSchema = z
  .object({
    date: z
      .string()
      .refine((value) => isoDayToUnix(value) !== null, 'validation.dateRequired')
      .refine((value) => value <= todayIso(), 'validation.stockSplitDayInvalid'),
    numerator: ratioPart,
    denominator: ratioPart,
  })
  .refine((values) => Number(values.numerator) !== Number(values.denominator), {
    message: 'validation.stockSplitRatioInvalid',
    path: ['denominator'],
  });

type FormValues = z.infer<typeof formSchema>;

const EMPTY_FORM: FormValues = { date: '', numerator: '', denominator: '' };

const STATUS_BADGE = {
  pending: 'dark',
  applied: 'default',
  dismissed: 'outline',
} as const satisfies Record<StockSplit['status'], 'dark' | 'default' | 'outline'>;

interface StockSplitsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  ticker: string;
  /** The ticker's splits, every status. */
  splits: readonly StockSplit[];
}

/**
 * The ticker's splits with what can still be done to each (spec D9: pending → recalculate or
 * already accounted for, applied → undo, dismissed → recalculate after all, a manual one not
 * applied → delete) and the form for a split Yahoo did not report (D11), which is recorded and
 * applied in one step.
 */
export function StockSplitsDialog({ open, onOpenChange, ticker, splits }: StockSplitsDialogProps) {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { apply, revert, dismiss, record, remove } = useStockSplitMutations();
  const busy =
    apply.isPending ||
    revert.isPending ||
    dismiss.isPending ||
    record.isPending ||
    remove.isPending;

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: EMPTY_FORM,
  });

  useEffect(() => {
    if (open) form.reset(EMPTY_FORM);
  }, [open, form]);

  const rows = [...splits].sort((a, b) => b.effectiveDay - a.effectiveDay);

  async function onSubmit(values: FormValues) {
    const payload = insertStockSplitSchema.parse({
      ticker,
      effectiveDay: isoDayToUnix(values.date),
      numerator: String(Number(values.numerator)),
      denominator: String(Number(values.denominator)),
    });
    try {
      await record.mutateAsync(payload);
      onOpenChange(false);
    } catch {
      // The mutation already showed the error; the form stays open to correct it.
    }
  }

  /** The row's buttons; `variables` marks the one that is running. */
  const rowActions = (split: StockSplit) => {
    const running = (mutation: { isPending: boolean; variables?: string }) =>
      mutation.isPending && mutation.variables === split.id;
    return (
      <div className="inline-flex gap-1">
        {(split.status === 'pending' || split.status === 'dismissed') && (
          <Button
            variant="outline"
            size="sm"
            disabled={busy}
            loading={running(apply)}
            onClick={() => apply.mutate(split.id)}
          >
            {t('splits.actions.apply')}
          </Button>
        )}
        {split.status === 'pending' && (
          <Button
            variant="ghost"
            size="sm"
            disabled={busy}
            loading={running(dismiss)}
            onClick={() => dismiss.mutate(split.id)}
          >
            {t('splits.actions.dismiss')}
          </Button>
        )}
        {split.status === 'applied' && (
          <Button
            variant="ghost"
            size="sm"
            disabled={busy}
            loading={running(revert)}
            onClick={() => revert.mutate(split.id)}
          >
            {t('splits.actions.revert')}
          </Button>
        )}
        {split.source === 'manual' && split.status !== 'applied' && (
          <Button
            variant="danger"
            size="sm"
            disabled={busy}
            loading={running(remove)}
            onClick={() => remove.mutate(split.id)}
          >
            {t('splits.actions.delete')}
          </Button>
        )}
      </div>
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[calc(100vw-48px)] max-w-[760px]">
        <DialogHeader>
          <DialogTitle>{t('splits.dialog.title', { ticker })}</DialogTitle>
          <DialogDescription>
            {rows.length > 0 ? t('splits.dialog.description') : t('splits.dialog.empty')}
          </DialogDescription>
        </DialogHeader>

        {rows.length > 0 && (
          <div className="-mx-[27px]">
            <Table containerClassName="max-h-[300px] overflow-y-auto">
              {/* The head's hairline is an inset shadow on the cells: a collapsed border would scroll away with the rows. */}
              <TableHeader className="sticky top-0 z-[1] bg-well [&_th]:shadow-[inset_0_-1px_0_var(--line)] [&_tr]:border-b-0">
                <TableRow>
                  <TableHead>{t('splits.columns.date')}</TableHead>
                  <TableHead>{t('splits.columns.ratio')}</TableHead>
                  <TableHead>{t('splits.columns.source')}</TableHead>
                  <TableHead>{t('splits.columns.status')}</TableHead>
                  <TableHead className="text-right">{t('splits.columns.count')}</TableHead>
                  <TableHead>
                    <span className="sr-only">{tc('labels.actions')}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.map((split) => (
                  <TableRow key={split.id} className="h-[50px]">
                    <TableCell className="whitespace-nowrap">
                      {fmt.day(split.effectiveDay)}
                    </TableCell>
                    <TableCell className="font-650 text-ink num">
                      {ratioText(split.numerator, split.denominator)}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      {t(`splits.source.${split.source}`)}
                    </TableCell>
                    <TableCell>
                      <Badge variant={STATUS_BADGE[split.status]}>
                        {t(`splits.status.${split.status}`)}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right num">
                      {split.status === 'applied'
                        ? (split.appliedCount ?? split.preSplitCount)
                        : split.preSplitCount}
                    </TableCell>
                    <TableCell className="whitespace-nowrap text-right">
                      {rowActions(split)}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}

        <section className="border-t border-line-soft pt-5">
          <h3 className="m-0 text-h3 text-ink">{t('splits.form.title')}</h3>
          <p className="mb-4 mt-1 text-table text-ink-3">{t('splits.form.description')}</p>
          <Form {...form}>
            <form
              id="record-split-form"
              onSubmit={form.handleSubmit(onSubmit)}
              className="grid grid-cols-3 gap-4"
            >
              <FormField
                control={form.control}
                name="date"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('splits.form.date')}</FormLabel>
                    <FormControl>
                      <Input type="date" max={todayIso()} {...field} />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="numerator"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('splits.form.numerator')}</FormLabel>
                    <FormControl>
                      <Input type="number" inputMode="numeric" min={1} step={1} {...field} />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="denominator"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('splits.form.denominator')}</FormLabel>
                    <FormControl>
                      <Input type="number" inputMode="numeric" min={1} step={1} {...field} />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </form>
          </Form>
        </section>

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.close')}
          </Button>
          <Button type="submit" form="record-split-form" disabled={busy} loading={record.isPending}>
            {t('splits.form.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
