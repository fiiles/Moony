import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { useTranslation } from 'react-i18next';
import { MIN_PASSWORD_LENGTH, setupSchema } from '@shared/schema';
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
import { Input } from '@/components/ui/input';
import { PasswordInput } from '@/components/auth/PasswordInput';
import { cn } from '@/lib/utils';
import type { FirstRunFlow, SetupFormValues } from '@/mobile/hooks/use-first-run';
import { StepHead } from './StepHead';
import { field48, fieldMessage, primaryButton, textLink } from './styles';

const EMPTY: SetupFormValues = {
  name: '',
  surname: '',
  email: '',
  password: '',
  confirmPassword: '',
};

/**
 * Simple length and variety strength hint: guidance, not a gate. 'weak' means "too short" (its
 * copy says so), so it applies only below the minimum length. As the desktop wizard's.
 */
function passwordStrength(password: string): 'weak' | 'fair' | 'strong' | null {
  if (!password) return null;
  const variety = [/[a-z]/, /[A-Z]/, /\d/, /[^\w\s]/].filter((re) => re.test(password)).length;
  if (password.length < MIN_PASSWORD_LENGTH) return 'weak';
  if (password.length >= 14 && variety >= 3) return 'strong';
  return 'fair';
}

/**
 * Účet: name, surname, an optional e-mail (only for the person's own orientation) and the password
 * that encrypts the database. "Pokračovat" prepares the keys; the recovery key arrives and the flow
 * moves to its step. Validation messages show under their field, one column of 48 pt fields.
 */
export function AccountStep({ flow }: { flow: FirstRunFlow }) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const form = useForm<SetupFormValues>({
    resolver: zodResolver(setupSchema),
    // Back from the recovery key: what was typed is still there
    defaultValues: flow.accountDraft ?? EMPTY,
  });
  const pending = flow.pending.setup;
  const strength = passwordStrength(form.watch('password'));

  // Back to Soukromí: what was typed comes back when the person returns to this step
  const goBack = () => {
    flow.setAccountDraft(form.getValues());
    flow.goBack();
  };

  return (
    <div data-testid="first-run-account">
      <StepHead title={t('onboarding.account.title')} lead={t('onboarding.account.subtitle')} />
      <Form {...form}>
        <form
          onSubmit={form.handleSubmit(flow.submitAccount)}
          aria-busy={pending}
          className="space-y-3"
        >
          <FormField
            control={form.control}
            name="name"
            render={({ field }) => (
              <FormItem>
                <FormLabel>{t('onboarding.account.name')}</FormLabel>
                <FormControl>
                  <Input
                    autoComplete="given-name"
                    autoCapitalize="words"
                    autoCorrect="off"
                    enterKeyHint="next"
                    className={field48}
                    readOnly={pending}
                    {...field}
                  />
                </FormControl>
                <FormMessage className={fieldMessage} />
              </FormItem>
            )}
          />
          <FormField
            control={form.control}
            name="surname"
            render={({ field }) => (
              <FormItem>
                <FormLabel>{t('onboarding.account.surname')}</FormLabel>
                <FormControl>
                  <Input
                    autoComplete="family-name"
                    autoCapitalize="words"
                    autoCorrect="off"
                    enterKeyHint="next"
                    className={field48}
                    readOnly={pending}
                    {...field}
                  />
                </FormControl>
                <FormMessage className={fieldMessage} />
              </FormItem>
            )}
          />
          <FormField
            control={form.control}
            name="email"
            render={({ field }) => (
              <FormItem>
                <FormLabel className="flex justify-between">
                  <span>{t('onboarding.account.email')}</span>
                  <span className="font-500 text-ink-5">{tc('labels.optional')}</span>
                </FormLabel>
                <FormControl>
                  {/* Text, not `email`: the schema validates, and the browser's own message would not be translated */}
                  <Input
                    type="text"
                    inputMode="email"
                    autoComplete="email"
                    autoCapitalize="none"
                    autoCorrect="off"
                    spellCheck={false}
                    enterKeyHint="next"
                    placeholder={t('onboarding.account.emailPlaceholder')}
                    className={field48}
                    readOnly={pending}
                    {...field}
                  />
                </FormControl>
                <FormMessage className={fieldMessage} />
              </FormItem>
            )}
          />
          <FormField
            control={form.control}
            name="password"
            render={({ field }) => (
              <FormItem>
                <FormLabel>{t('onboarding.account.password')}</FormLabel>
                <FormControl>
                  <PasswordInput
                    touch
                    autoComplete="new-password"
                    enterKeyHint="next"
                    className={field48}
                    readOnly={pending}
                    {...field}
                  />
                </FormControl>
                {strength && (
                  <FormDescription
                    className={cn(
                      'text-m-sub',
                      strength === 'weak' && 'text-loss',
                      strength === 'strong' && 'text-gain'
                    )}
                  >
                    {t(`onboarding.account.strength.${strength}`, { min: MIN_PASSWORD_LENGTH })}
                  </FormDescription>
                )}
                <FormMessage className={fieldMessage} />
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
                  <PasswordInput
                    touch
                    autoComplete="new-password"
                    enterKeyHint="go"
                    className={field48}
                    readOnly={pending}
                    {...field}
                  />
                </FormControl>
                <FormMessage className={fieldMessage} />
              </FormItem>
            )}
          />
          <div className="pt-2">
            <Button type="submit" className={primaryButton} disabled={pending} loading={pending}>
              {tc('buttons.continue')}
            </Button>
          </div>
        </form>
      </Form>
      <button type="button" className={textLink} onClick={goBack} disabled={pending}>
        {tc('buttons.back')}
      </button>
    </div>
  );
}
