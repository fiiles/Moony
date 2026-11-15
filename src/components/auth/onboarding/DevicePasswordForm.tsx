import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import type { z } from 'zod';
import { useTranslation } from 'react-i18next';
import { MIN_PASSWORD_LENGTH } from '@shared/schema';
import { Button } from '@/components/ui/button';
import {
  Form,
  FormControl,
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { PasswordInput } from '@/components/auth/PasswordInput';
import { cn } from '@/lib/utils';
import { passwordStrength } from '@/utils/password-strength';
import { devicePasswordSchema } from '@/utils/form-schemas';

interface DevicePasswordFormProps {
  /** The keys are being prepared: the submit shows its spinner. */
  pending: boolean;
  onSubmit: (password: string) => void;
  onBack: () => void;
}

/**
 * The join path's account step (spec 2026-11-14 §5.1): only this device's password, twice, with
 * the wizard's strength hint. Name, surname and e-mail come from the synced profile.
 */
export function DevicePasswordForm({ pending, onSubmit, onBack }: DevicePasswordFormProps) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const form = useForm<z.infer<typeof devicePasswordSchema>>({
    resolver: zodResolver(devicePasswordSchema),
    defaultValues: { password: '', confirmPassword: '' },
  });
  const strength = passwordStrength(form.watch('password'));

  return (
    <Form {...form}>
      <form
        onSubmit={form.handleSubmit((data) => onSubmit(data.password))}
        className="space-y-4"
        data-testid="onboarding-device-password"
      >
        <div className="grid grid-cols-2 gap-3">
          <FormField
            control={form.control}
            name="password"
            render={({ field }) => (
              <FormItem>
                <FormLabel>{t('onboarding.account.password')}</FormLabel>
                <FormControl>
                  <PasswordInput autoFocus autoComplete="new-password" {...field} />
                </FormControl>
                {strength && (
                  <FormDescription
                    className={cn(
                      strength === 'weak' && 'text-loss',
                      strength === 'strong' && 'text-gain'
                    )}
                  >
                    {t(`onboarding.account.strength.${strength}`, { min: MIN_PASSWORD_LENGTH })}
                  </FormDescription>
                )}
                <FormMessage />
              </FormItem>
            )}
          />
          <FormField
            control={form.control}
            name="confirmPassword"
            render={({ field }) => (
              <FormItem>
                <FormLabel>{t('onboarding.account.confirmPassword')}</FormLabel>
                <FormControl>
                  <PasswordInput autoComplete="new-password" {...field} />
                </FormControl>
                <FormMessage />
              </FormItem>
            )}
          />
        </div>
        <div className="mt-2 flex items-center justify-between">
          <Button type="button" variant="ghost" disabled={pending} onClick={onBack}>
            {tc('buttons.back')}
          </Button>
          <Button
            type="submit"
            disabled={pending}
            loading={pending}
            data-testid="onboarding-device-password-continue"
          >
            {tc('buttons.continue')}
          </Button>
        </div>
      </form>
    </Form>
  );
}
