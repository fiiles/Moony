import { useState, useEffect } from 'react';
import { useForm, useFieldArray } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useMutation, useQueryClient, useQuery } from '@tanstack/react-query';
import {
  insertRealEstateSchema,
  type InsertRealEstate,
  type Loan,
  type RealEstate,
  type InsurancePolicy,
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
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Plus, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Textarea } from '@/components/ui/textarea';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Checkbox } from '@/components/ui/checkbox';
import { FormSection } from '@/components/ui/form-section';
import { useCurrency } from '@/lib/currency';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { useFormat } from '@/lib/use-format';
import { realEstateApi, loansApi, insuranceApi, portfolioApi } from '@/lib/tauri-api';
import { isoDateFromUtcTimestamp, todayIsoUtc, utcDayStart } from '@/utils/period';
import { useTranslation } from 'react-i18next';

/**
 * The shared schema, with the purchase day as the ISO string an `<input type="date">` holds
 * ('' = unknown): it becomes the UTC-midnight epoch on submit (ADR 0008). A day after today is
 * refused here with a visible message, because native `max` validation can block a submit
 * silently in some webviews; the backend refuses it too.
 */
const realEstateFormSchema = insertRealEstateSchema.extend({
  purchaseDate: z
    .string()
    .optional()
    .refine((iso) => !iso || iso <= todayIsoUtc(), 'validation.purchaseDateInFuture'),
});
type RealEstateFormValues = z.infer<typeof realEstateFormSchema>;

const purchaseDateInput = (realEstate?: RealEstate) =>
  typeof realEstate?.purchaseDate === 'number'
    ? isoDateFromUtcTimestamp(realEstate.purchaseDate)
    : '';

interface AddRealEstateModalProps {
  realEstate?: RealEstate;
  trigger?: React.ReactNode;
  /** Controlled mode (opened from a menu): no trigger is rendered. */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}

export function AddRealEstateModal({
  realEstate,
  trigger,
  open: controlledOpen,
  onOpenChange,
}: AddRealEstateModalProps) {
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const [internalOpen, setInternalOpen] = useState(false);
  const controlled = controlledOpen !== undefined;
  const open = controlled ? controlledOpen : internalOpen;
  const setOpen = (next: boolean) => {
    if (!controlled) setInternalOpen(next);
    onOpenChange?.(next);
  };
  const queryClient = useQueryClient();
  const { currencyCode: userCurrency } = useCurrency();
  const fmt = useFormat();

  const form = useForm<RealEstateFormValues>({
    resolver: zodResolver(realEstateFormSchema),
    defaultValues: {
      name: realEstate?.name || '',
      address: realEstate?.address || '',
      type: realEstate?.type || 'personal',
      purchasePrice: realEstate?.purchasePrice?.toString() || '0',
      purchasePriceCurrency: realEstate?.purchasePriceCurrency || userCurrency,
      purchaseDate: purchaseDateInput(realEstate),
      marketPrice: realEstate?.marketPrice?.toString() || '0',
      marketPriceCurrency: realEstate?.marketPriceCurrency || userCurrency,
      monthlyRent: realEstate?.monthlyRent?.toString() || '0',
      monthlyRentCurrency: realEstate?.monthlyRentCurrency || userCurrency,
      recurringCosts:
        realEstate?.recurringCosts?.map((cost) => ({
          ...cost,
          currency: cost.currency || userCurrency,
        })) || [],
      photos: realEstate?.photos || [],
      notes: realEstate?.notes || '',
    },
  });

  // Reset form when realEstate changes or modal opens
  useEffect(() => {
    if (open && realEstate) {
      form.reset({
        name: realEstate.name,
        address: realEstate.address,
        type: realEstate.type,
        purchasePrice: realEstate.purchasePrice.toString(),
        purchasePriceCurrency: realEstate.purchasePriceCurrency || userCurrency,
        purchaseDate: purchaseDateInput(realEstate),
        marketPrice: realEstate.marketPrice.toString(),
        marketPriceCurrency: realEstate.marketPriceCurrency || userCurrency,
        monthlyRent: realEstate.monthlyRent?.toString() || '0',
        monthlyRentCurrency: realEstate.monthlyRentCurrency || userCurrency,
        recurringCosts:
          realEstate.recurringCosts?.map((cost) => ({
            ...cost,
            currency: cost.currency || userCurrency,
          })) || [],
        photos: realEstate.photos || [],
        notes: realEstate.notes || '',
      });
    } else if (open && !realEstate) {
      form.reset({
        name: '',
        address: '',
        type: 'personal',
        purchasePrice: '0',
        purchasePriceCurrency: userCurrency,
        purchaseDate: '',
        marketPrice: '0',
        marketPriceCurrency: userCurrency,
        monthlyRent: '0',
        monthlyRentCurrency: userCurrency,
        recurringCosts: [],
        photos: [],
        notes: '',
      });
    }
  }, [open, realEstate, form, userCurrency]);

  const {
    fields: costFields,
    append: appendCost,
    remove: removeCost,
  } = useFieldArray({
    control: form.control,
    name: 'recurringCosts',
  });

  // Fetch available loans (not linked to other real estate)
  const { data: availableLoans } = useQuery<Loan[]>({
    queryKey: ['available-loans'],
    queryFn: () => loansApi.getAvailable(),
    enabled: open,
  });

  // Fetch linked loans if editing
  const { data: linkedLoans } = useQuery<Loan[]>({
    queryKey: ['real-estate-loans', realEstate?.id],
    queryFn: () => (realEstate?.id ? realEstateApi.getLoans(realEstate.id) : Promise.resolve([])),
    enabled: !!realEstate?.id && open,
  });

  const [selectedLoanIds, setSelectedLoanIds] = useState<string[]>([]);
  const [selectedInsuranceIds, setSelectedInsuranceIds] = useState<string[]>([]);

  // Update selectedLoanIds when linkedLoans are fetched
  useEffect(() => {
    if (linkedLoans) {
      setSelectedLoanIds(linkedLoans.map((l) => l.id));
    } else {
      setSelectedLoanIds([]);
    }
  }, [linkedLoans]);

  // Fetch available insurances (not linked to any real estate)
  const { data: availableInsurances } = useQuery<InsurancePolicy[]>({
    queryKey: ['available-insurances'],
    queryFn: () => insuranceApi.getAvailable(),
    enabled: open,
  });

  // Fetch linked insurances if editing
  const { data: linkedInsurances } = useQuery<InsurancePolicy[]>({
    queryKey: ['real-estate-insurances', realEstate?.id],
    queryFn: () =>
      realEstate?.id ? realEstateApi.getInsurances(realEstate.id) : Promise.resolve([]),
    enabled: !!realEstate?.id && open,
  });

  // Update selectedInsuranceIds when linkedInsurances are fetched
  useEffect(() => {
    if (linkedInsurances) {
      setSelectedInsuranceIds(linkedInsurances.map((i) => i.id));
    } else {
      setSelectedInsuranceIds([]);
    }
  }, [linkedInsurances]);

  const mutation = useMutation({
    mutationFn: async (data: InsertRealEstate) => {
      let savedRealEstate;
      if (realEstate) {
        savedRealEstate = await realEstateApi.update(realEstate.id, data);
      } else {
        savedRealEstate = await realEstateApi.create(data);
      }

      // Handle loan linking
      if (realEstate) {
        // Get current links (linkedLoans) and compare with selectedLoanIds
        const currentIds = linkedLoans?.map((l) => l.id) || [];
        const toAdd = selectedLoanIds.filter((id) => !currentIds.includes(id));
        const toRemove = currentIds.filter((id) => !selectedLoanIds.includes(id));

        await Promise.all([
          ...toAdd.map((loanId) => realEstateApi.linkLoan(savedRealEstate.id, loanId)),
          ...toRemove.map((loanId) => realEstateApi.unlinkLoan(savedRealEstate.id, loanId)),
        ]);
      } else {
        // Create mode - just link selected
        if (selectedLoanIds.length > 0) {
          await Promise.all(
            selectedLoanIds.map((loanId) => realEstateApi.linkLoan(savedRealEstate.id, loanId))
          );
        }
      }

      // Handle insurance linking
      if (realEstate) {
        // Get current links (linkedInsurances) and compare with selectedInsuranceIds
        const currentInsuranceIds = linkedInsurances?.map((i) => i.id) || [];
        const toAddInsurance = selectedInsuranceIds.filter(
          (id) => !currentInsuranceIds.includes(id)
        );
        const toRemoveInsurance = currentInsuranceIds.filter(
          (id) => !selectedInsuranceIds.includes(id)
        );

        await Promise.all([
          ...toAddInsurance.map((insuranceId) =>
            realEstateApi.linkInsurance(savedRealEstate.id, insuranceId)
          ),
          ...toRemoveInsurance.map((insuranceId) =>
            realEstateApi.unlinkInsurance(savedRealEstate.id, insuranceId)
          ),
        ]);
      } else {
        // Create mode - just link selected insurances
        if (selectedInsuranceIds.length > 0) {
          await Promise.all(
            selectedInsuranceIds.map((insuranceId) =>
              realEstateApi.linkInsurance(savedRealEstate.id, insuranceId)
            )
          );
        }
      }

      return savedRealEstate;
    },
    onSuccess: async (saved) => {
      queryClient.invalidateQueries({ queryKey: ['real-estate'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      // A changed market price writes an estimate: the value trace must show it.
      queryClient.invalidateQueries({ queryKey: ['real-estate-valuations', saved.id] });
      if (realEstate) {
        queryClient.invalidateQueries({ queryKey: ['real-estate', realEstate.id] });
        queryClient.invalidateQueries({ queryKey: ['real-estate-loans', realEstate.id] });
        queryClient.invalidateQueries({ queryKey: ['real-estate-insurances', realEstate.id] });
      }
      queryClient.invalidateQueries({ queryKey: ['available-insurances'] });
      setOpen(false);
      if (!realEstate) {
        form.reset();
        setSelectedLoanIds([]);
        setSelectedInsuranceIds([]);
      }
      toast(tc('status.success'), {
        description: realEstate ? t('toast.updated') : t('toast.added'),
      });
      // Today's snapshot carries the new value; the save already succeeded, so a failed
      // snapshot must not turn it into an error.
      try {
        await portfolioApi.recordSnapshot();
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      } catch (error) {
        console.error('Failed to record portfolio snapshot:', error);
      }
    },
    onError: (error) => {
      toast.error(tc('status.error'), { description: error.message });
    },
  });

  const onSubmit = (data: RealEstateFormValues) => {
    const formattedData: InsertRealEstate = {
      ...data,
      purchasePrice: (data.purchasePrice || '0').toString(),
      purchaseDate: data.purchaseDate ? utcDayStart(data.purchaseDate) : null,
      marketPrice: (data.marketPrice || '0').toString(),
      monthlyRent: data.monthlyRent?.toString() || null,
    };
    mutation.mutate(formattedData);
  };

  const watchType = form.watch('type');

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      {!controlled && (
        <DialogTrigger asChild>
          {trigger || (
            <Button>
              <Plus /> {t('modal.add.addButton')}
            </Button>
          )}
        </DialogTrigger>
      )}
      <DialogContent className="max-w-3xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{realEstate ? t('modal.add.editTitle') : t('modal.add.title')}</DialogTitle>
          <DialogDescription>
            {realEstate ? t('modal.add.editDescription') : t('modal.add.description')}
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            id="add-real-estate-form"
            onSubmit={form.handleSubmit(onSubmit)}
            className="space-y-6"
          >
            {/* Property Details Section */}
            <FormSection title={t('modal.add.propertyDetails')} first>
              <div className="grid gap-4">
                <div className="grid grid-cols-2 gap-4">
                  <FormField
                    control={form.control}
                    name="name"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('modal.add.name')}</FormLabel>
                        <FormControl>
                          <Input placeholder={t('modal.add.namePlaceholder')} {...field} />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  <FormField
                    control={form.control}
                    name="type"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('modal.add.type')}</FormLabel>
                        <Select onValueChange={field.onChange} defaultValue={field.value}>
                          <FormControl>
                            <SelectTrigger>
                              <SelectValue placeholder={t('modal.add.selectType')} />
                            </SelectTrigger>
                          </FormControl>
                          <SelectContent>
                            <SelectItem value="personal">{t('modal.add.personalUse')}</SelectItem>
                            <SelectItem value="investment">{t('modal.add.investment')}</SelectItem>
                          </SelectContent>
                        </Select>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                </div>

                <FormField
                  control={form.control}
                  name="address"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('modal.add.address')}</FormLabel>
                      <FormControl>
                        <Input placeholder={t('modal.add.addressPlaceholder')} {...field} />
                      </FormControl>
                      <FormMessage />
                    </FormItem>
                  )}
                />
              </div>
            </FormSection>

            {/* Financial Details Section */}
            <FormSection title={t('modal.add.financialDetails')}>
              <div className="grid gap-4">
                <div className="grid grid-cols-3 gap-4">
                  <FormField
                    control={form.control}
                    name="purchasePrice"
                    render={({ field }) => (
                      <FormItem className="col-span-2">
                        <FormLabel>{t('modal.add.purchasePrice')}</FormLabel>
                        <FormControl>
                          <Input type="number" step="0.01" {...field} />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  <FormField
                    control={form.control}
                    name="purchasePriceCurrency"
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

                <div className="grid grid-cols-3 gap-4">
                  <FormField
                    control={form.control}
                    name="purchaseDate"
                    render={({ field }) => (
                      <FormItem className="col-span-2">
                        <FormLabel className="flex justify-between">
                          <span>{t('modal.add.purchaseDate')}</span>
                          <span className="font-500 text-ink-5">{tc('labels.optional')}</span>
                        </FormLabel>
                        <FormControl>
                          <Input type="date" {...field} value={field.value ?? ''} />
                        </FormControl>
                        <FormDescription>{t('modal.add.purchaseDateHelp')}</FormDescription>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                </div>

                <div className="grid grid-cols-3 gap-4">
                  <FormField
                    control={form.control}
                    name="marketPrice"
                    render={({ field }) => (
                      <FormItem className="col-span-2">
                        <FormLabel>{t('modal.add.marketPrice')}</FormLabel>
                        <FormControl>
                          <Input type="number" step="0.01" {...field} />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  <FormField
                    control={form.control}
                    name="marketPriceCurrency"
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

                {watchType === 'investment' && (
                  <div className="grid grid-cols-3 gap-4">
                    <FormField
                      control={form.control}
                      name="monthlyRent"
                      render={({ field }) => (
                        <FormItem className="col-span-2">
                          <FormLabel>{t('modal.add.monthlyRent')}</FormLabel>
                          <FormControl>
                            <Input type="number" step="0.01" {...field} value={field.value || ''} />
                          </FormControl>
                          <FormMessage />
                        </FormItem>
                      )}
                    />
                    <FormField
                      control={form.control}
                      name="monthlyRentCurrency"
                      render={({ field }) => (
                        <FormItem>
                          <FormLabel>{tc('labels.currency')}</FormLabel>
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
                )}
              </div>
            </FormSection>

            {/* Recurring Costs Section */}
            <FormSection title={t('modal.add.recurringCosts')}>
              <div className="flex justify-end">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    appendCost({
                      name: '',
                      amount: 0,
                      frequency: 'monthly',
                      currency: userCurrency,
                    })
                  }
                >
                  <Plus className="h-4 w-4 mr-1" />
                  {t('modal.add.addCost')}
                </Button>
              </div>
              <div className="space-y-2">
                {costFields.map((field, index) => (
                  <div key={field.id} className="flex gap-2 items-end">
                    <FormField
                      control={form.control}
                      name={`recurringCosts.${index}.name`}
                      render={({ field }) => (
                        <FormItem className="flex-1">
                          <FormControl>
                            <Input placeholder={t('modal.add.costName')} {...field} />
                          </FormControl>
                        </FormItem>
                      )}
                    />
                    <FormField
                      control={form.control}
                      name={`recurringCosts.${index}.amount`}
                      render={({ field }) => (
                        <FormItem className="w-24">
                          <FormControl>
                            <Input
                              type="number"
                              {...field}
                              onChange={(e) => field.onChange(parseFloat(e.target.value))}
                            />
                          </FormControl>
                        </FormItem>
                      )}
                    />
                    <FormField
                      control={form.control}
                      name={`recurringCosts.${index}.currency`}
                      render={({ field }) => (
                        <FormItem className="w-24">
                          <FormControl>
                            <CurrencyCombobox
                              showName={false}
                              value={field.value || userCurrency}
                              onChange={field.onChange}
                            />
                          </FormControl>
                        </FormItem>
                      )}
                    />
                    <FormField
                      control={form.control}
                      name={`recurringCosts.${index}.frequency`}
                      render={({ field }) => (
                        <FormItem className="w-32">
                          <Select onValueChange={field.onChange} defaultValue={field.value}>
                            <FormControl>
                              <SelectTrigger>
                                <SelectValue />
                              </SelectTrigger>
                            </FormControl>
                            <SelectContent>
                              <SelectItem value="monthly">{t('modal.add.monthly')}</SelectItem>
                              <SelectItem value="quarterly">{t('modal.add.quarterly')}</SelectItem>
                              <SelectItem value="yearly">{t('modal.add.yearly')}</SelectItem>
                            </SelectContent>
                          </Select>
                        </FormItem>
                      )}
                    />
                    <Button
                      type="button"
                      variant="danger"
                      size="icon"
                      aria-label={t('modal.add.removeCost')}
                      onClick={() => removeCost(index)}
                    >
                      <Trash2 />
                    </Button>
                  </div>
                ))}
                {costFields.length === 0 && (
                  <p className="text-sm text-ink-3 text-center py-4">{t('modal.add.noCosts')}</p>
                )}
              </div>
            </FormSection>

            {/* Financing Section */}
            <FormSection title={t('modal.add.financing')}>
              <div className="space-y-2">
                <FormLabel>{t('modal.add.associatedLoans')}</FormLabel>
                <ScrollArea className="h-32 border rounded-r1 p-2">
                  {/* Show available loans and currently linked ones */}
                  {[...(availableLoans || []), ...(linkedLoans || [])].map((loan) => (
                    <div key={loan.id} className="flex items-center space-x-2 py-1">
                      <Checkbox
                        id={`loan-${loan.id}`}
                        checked={selectedLoanIds.includes(loan.id)}
                        onCheckedChange={(checked) => {
                          if (checked) {
                            setSelectedLoanIds([...selectedLoanIds, loan.id]);
                          } else {
                            setSelectedLoanIds(selectedLoanIds.filter((id) => id !== loan.id));
                          }
                        }}
                      />
                      <label
                        htmlFor={`loan-${loan.id}`}
                        className="text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70"
                      >
                        {loan.name} ({fmt.money(Number(loan.principal), loan.currency)})
                      </label>
                    </div>
                  ))}
                  {(!availableLoans || availableLoans.length === 0) &&
                    (!linkedLoans || linkedLoans.length === 0) && (
                      <p className="text-sm text-ink-3 text-center py-4">
                        {t('modal.add.noLoans')}
                      </p>
                    )}
                </ScrollArea>
                <p className="text-xs text-ink-3">{t('modal.add.loanRemark')}</p>
              </div>
            </FormSection>

            {/* Insurance Linking Section */}
            <FormSection title={t('modal.add.linkInsurances')}>
              <div className="space-y-2">
                <ScrollArea className="h-32 border rounded-r1 p-2">
                  {/* Show available insurances and currently linked ones */}
                  {[...(availableInsurances || []), ...(linkedInsurances || [])].map(
                    (insurance) => (
                      <div key={insurance.id} className="flex items-center space-x-2 py-1">
                        <Checkbox
                          id={`insurance-${insurance.id}`}
                          checked={selectedInsuranceIds.includes(insurance.id)}
                          onCheckedChange={(checked) => {
                            if (checked) {
                              setSelectedInsuranceIds([...selectedInsuranceIds, insurance.id]);
                            } else {
                              setSelectedInsuranceIds(
                                selectedInsuranceIds.filter((id) => id !== insurance.id)
                              );
                            }
                          }}
                        />
                        <label
                          htmlFor={`insurance-${insurance.id}`}
                          className="text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70"
                        >
                          {insurance.policyName} ({insurance.provider})
                        </label>
                      </div>
                    )
                  )}
                  {(!availableInsurances || availableInsurances.length === 0) &&
                    (!linkedInsurances || linkedInsurances.length === 0) && (
                      <p className="text-sm text-ink-3 text-center py-4">
                        {t('modal.add.noAvailableInsurances')}
                      </p>
                    )}
                </ScrollArea>
                <p className="text-xs text-ink-3">{t('modal.add.insuranceRemark')}</p>
              </div>
            </FormSection>

            {/* Additional Information Section */}
            <FormSection title={t('modal.add.additionalInfo')}>
              <FormField
                control={form.control}
                name="notes"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.add.notes')}</FormLabel>
                    <FormControl>
                      <Textarea
                        placeholder={tc('labels.notes')}
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
          <Button
            type="submit"
            form="add-real-estate-form"
            disabled={mutation.isPending}
            loading={mutation.isPending}
          >
            {mutation.isPending
              ? tc('status.saving')
              : realEstate
                ? t('modal.add.updateProperty')
                : t('modal.add.createProperty')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
