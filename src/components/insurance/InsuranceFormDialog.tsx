import { useState, useEffect } from 'react';
import { useForm, useFieldArray } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import type { InsurancePolicy } from '@shared/schema';
import {
  PAYMENT_FREQUENCIES,
  parsePaymentFrequency,
  type PaymentFrequency,
} from '@shared/calculations';
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Textarea } from '@/components/ui/textarea';
import { FormSection } from '@/components/ui/form-section';
import { toast } from 'sonner';
import { useQueryClient, useMutation } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import { useCurrency } from '@/lib/currency';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { insuranceApi } from '@/lib/tauri-api';
import { useTranslation } from 'react-i18next';
import { translateApiError } from '@/lib/translate-api-error';
import {
  insurancePaymentFields,
  insurancePaymentIssues,
  normalizeInsurancePayments,
} from '@/utils/insurance-payment';

// Form-specific schema with Date objects
const insuranceFormSchema = z
  .object({
    type: z.string().min(1, 'validation.insuranceTypeRequired'),
    provider: z.string().min(1, 'validation.providerRequired'),
    policyName: z.string().min(1, 'validation.policyNameRequired'),
    policyNumber: z.string().optional(),
    startDate: z.date({ error: 'validation.dateRequired' }),
    endDate: z.date().optional().nullable(),
    paymentFrequency: z.enum(PAYMENT_FREQUENCIES, { error: 'validation.paymentFrequencyInvalid' }),
    oneTimePayment: z.string().optional(),
    oneTimePaymentCurrency: z.string().optional(),
    regularPayment: z.string().optional(),
    regularPaymentCurrency: z.string().optional(),
    limits: z
      .array(
        z.object({
          title: z.string(),
          amount: z.number(),
          currency: z.string(),
        })
      )
      .optional(),
    notes: z.string().optional(),
  })
  // Only the payment inputs shown for the chosen frequency are validated.
  .superRefine((data, ctx) => {
    for (const issue of insurancePaymentIssues(
      data.paymentFrequency,
      data.oneTimePayment,
      data.regularPayment
    )) {
      ctx.addIssue({ code: 'custom', message: issue.key, path: [issue.path] });
    }
  });

type InsuranceFormData = z.infer<typeof insuranceFormSchema>;

interface InsuranceFormDialogProps {
  policy?: InsurancePolicy;
  trigger?: React.ReactNode;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}

export function InsuranceFormDialog({
  policy,
  trigger,
  open,
  onOpenChange,
}: InsuranceFormDialogProps) {
  const { t } = useTranslation('insurance');
  const { t: tc } = useTranslation('common');
  const [internalOpen, setInternalOpen] = useState(false);
  const queryClient = useQueryClient();
  const { currencyCode: userCurrency } = useCurrency();

  const isControlled = open !== undefined;
  const isOpen = isControlled ? open : internalOpen;
  const setIsOpen = isControlled ? onOpenChange : setInternalOpen;

  const form = useForm<InsuranceFormData>({
    resolver: zodResolver(insuranceFormSchema),
    defaultValues: {
      type: policy?.type || 'life',
      provider: policy?.provider || '',
      policyName: policy?.policyName || '',
      policyNumber: policy?.policyNumber || '',
      startDate: policy?.startDate ? new Date(policy.startDate * 1000) : new Date(),
      endDate: policy?.endDate ? new Date(policy.endDate * 1000) : undefined,
      paymentFrequency: (parsePaymentFrequency(policy?.paymentFrequency) ??
        'monthly') as PaymentFrequency,
      oneTimePayment: policy?.oneTimePayment?.toString() || undefined,
      oneTimePaymentCurrency:
        (policy as InsurancePolicy & { oneTimePaymentCurrency?: string })?.oneTimePaymentCurrency ||
        userCurrency,
      regularPayment: policy?.regularPayment?.toString() || '0',
      regularPaymentCurrency:
        (policy as InsurancePolicy & { regularPaymentCurrency?: string })?.regularPaymentCurrency ||
        userCurrency,
      limits: policy?.limits || [],
      notes: policy?.notes || '',
    },
  });

  const { fields, append, remove } = useFieldArray({
    control: form.control,
    name: 'limits',
  });

  // Reset form when policy changes or dialog opens
  useEffect(() => {
    if (isOpen) {
      form.reset({
        type: policy?.type || 'life',
        provider: policy?.provider || '',
        policyName: policy?.policyName || '',
        policyNumber: policy?.policyNumber || '',
        startDate: policy?.startDate ? new Date(policy.startDate * 1000) : new Date(),
        endDate: policy?.endDate ? new Date(policy.endDate * 1000) : undefined,
        paymentFrequency: (parsePaymentFrequency(policy?.paymentFrequency) ??
          'monthly') as PaymentFrequency,
        oneTimePayment: policy?.oneTimePayment?.toString() || undefined,
        oneTimePaymentCurrency:
          (policy as InsurancePolicy & { oneTimePaymentCurrency?: string })
            ?.oneTimePaymentCurrency || userCurrency,
        regularPayment: policy?.regularPayment?.toString() || '0',
        regularPaymentCurrency:
          (policy as InsurancePolicy & { regularPaymentCurrency?: string })
            ?.regularPaymentCurrency || userCurrency,
        limits: policy?.limits || [],
        notes: policy?.notes || '',
      });
    }
  }, [isOpen, policy, form, userCurrency]);

  const createMutation = useMutation({
    mutationFn: async (data: InsuranceFormData) => {
      // Convert Date to timestamp
      const payload = {
        ...data,
        startDate: Math.floor(data.startDate.getTime() / 1000),
        endDate: data.endDate ? Math.floor(data.endDate.getTime() / 1000) : undefined,
      };
      return insuranceApi.create(payload);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['insurance'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      setIsOpen?.(false);
      toast(t('toast.added'));
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  const updateMutation = useMutation({
    mutationFn: async (data: InsuranceFormData) => {
      const payload = {
        ...data,
        startDate: Math.floor(data.startDate.getTime() / 1000),
        endDate: data.endDate ? Math.floor(data.endDate.getTime() / 1000) : undefined,
      };
      return insuranceApi.update(policy!.id, payload);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['insurance'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      setIsOpen?.(false);
      toast(t('toast.updated'));
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  // A stored recurring policy with a one-time amount keeps that field (optional) instead of
  // silently losing the value.
  const hasStoredOneTime = !!policy && Number(policy.oneTimePayment) > 0;
  const paymentFrequency = form.watch('paymentFrequency');
  const paymentFields = insurancePaymentFields(paymentFrequency, hasStoredOneTime);

  const onSubmit = (formData: InsuranceFormData) => {
    // Inputs hidden for the chosen frequency are reset, never sent as stale numbers.
    const data = {
      ...formData,
      ...normalizeInsurancePayments({
        frequency: formData.paymentFrequency,
        oneTimePayment: formData.oneTimePayment,
        regularPayment: formData.regularPayment,
        keepOneTime: hasStoredOneTime,
      }),
    };
    if (policy) {
      updateMutation.mutate(data);
    } else {
      createMutation.mutate(data);
    }
  };

  const isEditMode = !!policy;
  const mutation = isEditMode ? updateMutation : createMutation;

  return (
    <Dialog open={isOpen} onOpenChange={setIsOpen}>
      {trigger && <DialogTrigger asChild>{trigger}</DialogTrigger>}
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{policy ? t('form.editTitle') : t('form.addTitle')}</DialogTitle>
          <DialogDescription>
            {policy ? t('modal.editDescription') : t('modal.addDescription')}
          </DialogDescription>
        </DialogHeader>

        {/* Only the body scrolls: header and footer stay in view, and the scrollbar shows that
            there is more below. */}
        <Form {...form}>
          <form
            id="insurance-form"
            onSubmit={form.handleSubmit(onSubmit)}
            className="max-h-[70vh] space-y-6 overflow-y-auto px-1"
          >
            {/* Basic Information Section */}
            <FormSection title={t('modal.basicInfo')} first>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <FormField
                  control={form.control}
                  name="type"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('form.type')}</FormLabel>
                      <Select onValueChange={field.onChange} defaultValue={field.value}>
                        <FormControl>
                          <SelectTrigger>
                            <SelectValue placeholder={t('modal.selectType')} />
                          </SelectTrigger>
                        </FormControl>
                        <SelectContent>
                          <SelectItem value="life">{t('types.life')}</SelectItem>
                          <SelectItem value="travel">{t('types.travel')}</SelectItem>
                          <SelectItem value="accident">{t('types.accident')}</SelectItem>
                          <SelectItem value="property">{t('types.property')}</SelectItem>
                          <SelectItem value="liability">{t('types.liability')}</SelectItem>
                          <SelectItem value="other">{t('types.other')}</SelectItem>
                        </SelectContent>
                      </Select>
                      <FormMessage />
                    </FormItem>
                  )}
                />

                <FormField
                  control={form.control}
                  name="policyName"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('modal.policyName')} *</FormLabel>
                      <FormControl>
                        <Input {...field} placeholder={t('modal.policyNamePlaceholder')} />
                      </FormControl>
                      <FormMessage />
                    </FormItem>
                  )}
                />

                <FormField
                  control={form.control}
                  name="provider"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('modal.provider')} *</FormLabel>
                      <FormControl>
                        <Input {...field} placeholder={t('modal.providerPlaceholder')} />
                      </FormControl>
                      <FormMessage />
                    </FormItem>
                  )}
                />

                <FormField
                  control={form.control}
                  name="policyNumber"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('modal.policyNumber')}</FormLabel>
                      <FormControl>
                        <Input {...field} placeholder={t('modal.policyNumberPlaceholder')} />
                      </FormControl>
                      <FormMessage />
                    </FormItem>
                  )}
                />
              </div>
            </FormSection>

            {/* Payment Details Section */}
            <FormSection title={t('modal.paymentDetails')}>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <FormField
                  control={form.control}
                  name="paymentFrequency"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('modal.paymentFrequency')}</FormLabel>
                      <Select onValueChange={field.onChange} defaultValue={field.value}>
                        <FormControl>
                          <SelectTrigger>
                            <SelectValue placeholder={t('modal.selectFrequency')} />
                          </SelectTrigger>
                        </FormControl>
                        <SelectContent>
                          {PAYMENT_FREQUENCIES.map((frequency) => (
                            <SelectItem key={frequency} value={frequency}>
                              {t(`modal.frequency.${frequency}`)}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                      <FormMessage />
                    </FormItem>
                  )}
                />

                {paymentFields.showOneTime && (
                  <div className="col-span-2">
                    <FormLabel>
                      {paymentFields.oneTimeRequired
                        ? `${t('modal.oneTimePaymentRequired')} *`
                        : t('modal.oneTimePayment')}
                    </FormLabel>
                    <div className="grid grid-cols-3 gap-2 mt-2">
                      <FormField
                        control={form.control}
                        name="oneTimePayment"
                        render={({ field }) => (
                          <FormItem className="col-span-2">
                            <FormControl>
                              <Input
                                {...field}
                                value={field.value ?? ''}
                                type="number"
                                step="0.01"
                                placeholder="0.00"
                              />
                            </FormControl>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                      <FormField
                        control={form.control}
                        name="oneTimePaymentCurrency"
                        render={({ field }) => (
                          <FormItem>
                            <FormControl>
                              <CurrencyCombobox
                                value={field.value ?? undefined}
                                onChange={field.onChange}
                              />
                            </FormControl>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                    </div>
                  </div>
                )}

                {paymentFields.showRegular && (
                  <div className="col-span-2">
                    <FormLabel>{t('modal.regularPayment')} *</FormLabel>
                    <div className="grid grid-cols-3 gap-2 mt-2">
                      <FormField
                        control={form.control}
                        name="regularPayment"
                        render={({ field }) => (
                          <FormItem className="col-span-2">
                            <FormControl>
                              <Input {...field} type="number" step="0.01" />
                            </FormControl>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                      <FormField
                        control={form.control}
                        name="regularPaymentCurrency"
                        render={({ field }) => (
                          <FormItem>
                            <FormControl>
                              <CurrencyCombobox value={field.value} onChange={field.onChange} />
                            </FormControl>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                    </div>
                  </div>
                )}

                <FormField
                  control={form.control}
                  name="startDate"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('form.startDate')} *</FormLabel>
                      <FormControl>
                        <Input
                          type="date"
                          value={
                            field.value ? new Date(field.value).toISOString().split('T')[0] : ''
                          }
                          onChange={(e) =>
                            field.onChange(e.target.value ? new Date(e.target.value) : undefined)
                          }
                        />
                      </FormControl>
                      <FormMessage />
                    </FormItem>
                  )}
                />

                <FormField
                  control={form.control}
                  name="endDate"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('modal.endDateOptional')}</FormLabel>
                      <FormControl>
                        <Input
                          type="date"
                          value={
                            field.value ? new Date(field.value).toISOString().split('T')[0] : ''
                          }
                          onChange={(e) =>
                            field.onChange(e.target.value ? new Date(e.target.value) : null)
                          }
                        />
                      </FormControl>
                      <FormMessage />
                    </FormItem>
                  )}
                />
              </div>
            </FormSection>

            {/* Additional Information Section */}
            <FormSection title={t('modal.additionalInfo')}>
              <div className="space-y-2">
                <div className="flex justify-between items-center">
                  <FormLabel>{t('modal.coverageLimits')}</FormLabel>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() =>
                      append({ title: '', amount: 0, currency: userCurrency as string })
                    }
                  >
                    <Plus className="h-4 w-4 mr-2" />
                    {t('modal.addLimit')}
                  </Button>
                </div>
                {fields.map((field, index) => (
                  <div key={field.id} className="flex gap-2 items-end">
                    <FormField
                      control={form.control}
                      name={`limits.${index}.title` as const}
                      render={({ field }) => (
                        <FormItem className="flex-1">
                          <FormControl>
                            <Input {...field} placeholder={t('modal.limitTitle')} />
                          </FormControl>
                        </FormItem>
                      )}
                    />
                    <FormField
                      control={form.control}
                      name={`limits.${index}.amount` as const}
                      render={({ field }) => (
                        <FormItem className="w-32">
                          <FormControl>
                            <Input
                              {...field}
                              type="number"
                              onChange={(e) => field.onChange(parseFloat(e.target.value))}
                            />
                          </FormControl>
                        </FormItem>
                      )}
                    />
                    <FormField
                      control={form.control}
                      name={`limits.${index}.currency` as const}
                      render={({ field }) => (
                        <FormItem className="w-24">
                          <FormControl>
                            <CurrencyCombobox
                              showName={false}
                              value={field.value}
                              onChange={field.onChange}
                            />
                          </FormControl>
                        </FormItem>
                      )}
                    />
                    <Button
                      type="button"
                      variant="danger"
                      size="icon"
                      aria-label={t('modal.removeLimit')}
                      onClick={() => remove(index)}
                    >
                      <Trash2 />
                    </Button>
                  </div>
                ))}
              </div>

              <FormField
                control={form.control}
                name="notes"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('form.notes')}</FormLabel>
                    <FormControl>
                      <Textarea
                        {...field}
                        value={field.value ?? ''}
                        placeholder={t('modal.notesPlaceholder')}
                      />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </FormSection>
          </form>
        </Form>
        <DialogFooter className="border-t pt-4">
          <Button type="button" variant="ghost" onClick={() => setIsOpen?.(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form="insurance-form"
            disabled={mutation.isPending}
            loading={mutation.isPending}
          >
            {mutation.isPending
              ? tc('status.saving')
              : isEditMode
                ? tc('buttons.saveChanges')
                : t('modal.createPolicy')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
