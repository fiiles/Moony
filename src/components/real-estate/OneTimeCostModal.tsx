import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import {
  insertRealEstateOneTimeCostSchema,
  type InsertRealEstateOneTimeCost,
  type RealEstateOneTimeCost,
} from '@shared/schema';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog';
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Plus, Loader2 } from 'lucide-react';
import { FormSection } from '@/components/ui/form-section';
import { toast } from 'sonner';
import { Textarea } from '@/components/ui/textarea';
import { useCurrency } from '@/lib/currency';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { realEstateApi } from '@/lib/tauri-api';
import { useTranslation } from 'react-i18next';

interface OneTimeCostModalProps {
  realEstateId: string;
  cost?: RealEstateOneTimeCost;
  trigger?: React.ReactNode;
}

export function OneTimeCostModal({ realEstateId, cost, trigger }: OneTimeCostModalProps) {
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const [open, setOpen] = useState(false);
  const queryClient = useQueryClient();
  const { currencyCode: userCurrency } = useCurrency();
  const isEditMode = !!cost;

  const form = useForm<InsertRealEstateOneTimeCost>({
    resolver: zodResolver(insertRealEstateOneTimeCostSchema),
    defaultValues: {
      name: cost?.name || '',
      description: cost?.description || '',
      amount: cost?.amount?.toString() || '0',
      currency: cost?.currency || userCurrency,
      date: cost?.date ? new Date(cost.date * 1000) : new Date(),
      realEstateId,
    },
  });

  const createMutation = useMutation({
    mutationFn: async (data: InsertRealEstateOneTimeCost) => {
      const costData = {
        ...data,
        realEstateId,
        date: data.date
          ? Math.floor(new Date(data.date).getTime() / 1000)
          : Math.floor(Date.now() / 1000),
      };

      if (isEditMode) {
        return realEstateApi.updateCost(cost!.id, costData);
      } else {
        return realEstateApi.createCost(costData);
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['real-estate-costs', realEstateId] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      setOpen(false);
      if (!isEditMode) {
        form.reset({
          name: '',
          description: '',
          amount: '0',
          currency: userCurrency,
          date: new Date(),
          realEstateId,
        });
      }
      toast(tc('status.success'), {
        description: isEditMode ? t('toast.costUpdated') : t('toast.costAdded'),
      });
    },
    onError: (error) => {
      toast.error(tc('status.error'), { description: error.message });
    },
  });

  const onSubmit = (data: InsertRealEstateOneTimeCost) => {
    createMutation.mutate({
      ...data,
      amount: data.amount.toString(),
      realEstateId,
    });
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      {trigger ? (
        <DialogTrigger asChild>{trigger}</DialogTrigger>
      ) : (
        <DialogTrigger asChild>
          <Button size="sm">
            <Plus className="mr-2 h-4 w-4" /> {t('modal.cost.addCost')}
          </Button>
        </DialogTrigger>
      )}
      <DialogContent className="max-w-xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>
            {isEditMode ? t('modal.cost.editTitle') : t('modal.cost.title')}
          </DialogTitle>
          <DialogDescription>
            {isEditMode ? t('modal.cost.editDescription') : t('modal.cost.description')}
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            id="one-time-cost-form"
            onSubmit={form.handleSubmit(onSubmit)}
            className="space-y-6"
          >
            <FormSection title={t('modal.cost.costDetails')} first>
              <div className="grid gap-4">
                <FormField
                  control={form.control}
                  name="name"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('modal.cost.name')}</FormLabel>
                      <FormControl>
                        <Input placeholder={t('modal.cost.namePlaceholder')} {...field} />
                      </FormControl>
                      <FormMessage />
                    </FormItem>
                  )}
                />

                <div className="grid grid-cols-3 gap-4">
                  <FormField
                    control={form.control}
                    name="amount"
                    render={({ field }) => (
                      <FormItem className="col-span-2">
                        <FormLabel>{t('modal.cost.amount')}</FormLabel>
                        <FormControl>
                          <Input type="number" step="0.01" {...field} />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  <FormField
                    control={form.control}
                    name="currency"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{tc('labels.currency')}</FormLabel>
                        <FormControl>
                          <CurrencyCombobox value={field.value} onChange={field.onChange} />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                </div>

                <FormField
                  control={form.control}
                  name="date"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('modal.cost.date')}</FormLabel>
                      <FormControl>
                        <Input
                          type="date"
                          value={
                            field.value ? new Date(field.value).toISOString().split('T')[0] : ''
                          }
                          onChange={(e) => field.onChange(e.target.valueAsDate)}
                        />
                      </FormControl>
                      <FormMessage />
                    </FormItem>
                  )}
                />
              </div>

              <FormField
                control={form.control}
                name="description"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.cost.descriptionLabel')}</FormLabel>
                    <FormControl>
                      <Textarea
                        placeholder={t('modal.cost.descriptionPlaceholder')}
                        {...field}
                        value={field.value || ''}
                      />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </FormSection>
          </form>
        </Form>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => setOpen(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button type="submit" form="one-time-cost-form" disabled={createMutation.isPending}>
            {createMutation.isPending && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            {isEditMode ? t('modal.cost.updateCost') : t('modal.cost.addCost')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
