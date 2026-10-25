import { zodResolver } from '@hookform/resolvers/zod';
import { useForm } from 'react-hook-form';
import { useTranslation } from 'react-i18next';
import { z } from 'zod';
import { Button } from '@/components/ui/button';
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { useAuth } from '@/hooks/use-auth';
import { useSettingsMutations } from '@/hooks/use-settings-mutations';
import { SectionPage } from '@/mobile/components/SectionPage';

/**
 * The desktop `ProfileCard`'s keys and messages, except that the e-mail stays optional as it is in
 * the phone's first run (`setupSchema`): an empty one passes, a filled one must be an address.
 */
const profileSchema = z.object({
  name: z.string().min(1, 'validation.nameRequired'),
  surname: z.string().min(1, 'validation.surnameRequired'),
  email: z.string().email('validation.invalidEmail').or(z.literal('')),
});

type ProfileData = z.infer<typeof profileSchema>;

/** Fields and the primary button as on the lock screen (`Lock.tsx`): 48 pt, radius 10. */
const field48 = 'h-12 rounded-[10px] text-m-field';
const primaryButton =
  'h-auto min-h-12 whitespace-normal py-2 text-center w-full rounded-[10px] text-m-row';
const fieldMessage = 'text-left text-m-sub';

/**
 * Nastavení → Profil (spec §8.9): name, surname and e-mail as on the desktop's profile card. The
 * main currency is not here: it is a row of Nastavení next to the language (Jazyk a měna).
 */
export default function ProfileSection() {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const { user } = useAuth();
  const { updateProfile } = useSettingsMutations();

  const form = useForm<ProfileData>({
    resolver: zodResolver(profileSchema),
    defaultValues: {
      name: user?.name ?? '',
      surname: user?.surname ?? '',
      email: user?.email ?? '',
    },
  });

  return (
    <SectionPage title={t('settings.rows.profile')} lead={t('settings.profile.lead')}>
      <Form {...form}>
        <form
          onSubmit={form.handleSubmit((data) => updateProfile.mutate(data))}
          aria-busy={updateProfile.isPending}
          // The messages are zod's (translated), not the browser's bubble for `type="email"`
          noValidate
          className="space-y-4"
        >
          <FormField
            control={form.control}
            name="name"
            render={({ field }) => (
              <FormItem>
                <FormLabel>{tc('labels.name')}</FormLabel>
                <FormControl>
                  <Input
                    autoComplete="given-name"
                    autoCapitalize="words"
                    enterKeyHint="next"
                    className={field48}
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
                <FormLabel>{tc('labels.surname')}</FormLabel>
                <FormControl>
                  <Input
                    autoComplete="family-name"
                    autoCapitalize="words"
                    enterKeyHint="next"
                    className={field48}
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
                <FormLabel>{tc('labels.email')}</FormLabel>
                <FormControl>
                  <Input
                    type="email"
                    autoComplete="email"
                    autoCapitalize="none"
                    autoCorrect="off"
                    spellCheck={false}
                    enterKeyHint="done"
                    className={field48}
                    {...field}
                  />
                </FormControl>
                <FormMessage className={fieldMessage} />
              </FormItem>
            )}
          />
          <Button type="submit" className={primaryButton} loading={updateProfile.isPending}>
            {tc('buttons.saveChanges')}
          </Button>
        </form>
      </Form>
    </SectionPage>
  );
}
