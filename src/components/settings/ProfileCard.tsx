import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useTranslation } from 'react-i18next';
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
import { SettingsCard } from '@/components/settings/SettingsCard';
import { useAuth } from '@/hooks/use-auth';
import { useSettingsMutations } from '@/hooks/use-settings-mutations';

const profileSchema = z.object({
  name: z.string().min(1, 'validation.nameRequired'),
  surname: z.string().min(1, 'validation.surnameRequired'),
  email: z.string().email('validation.invalidEmail'),
});

type ProfileData = z.infer<typeof profileSchema>;

/** Settings → General → Profil: name and surname in one row, e-mail under them. */
export function ProfileCard() {
  const { t } = useTranslation('settings');
  const { user } = useAuth();
  const { updateProfile } = useSettingsMutations();

  const form = useForm<ProfileData>({
    resolver: zodResolver(profileSchema),
    defaultValues: {
      name: user?.name || '',
      surname: user?.surname || '',
      email: user?.email || '',
    },
  });

  return (
    <SettingsCard title={t('profile.title')} description={t('profile.description')}>
      <Form {...form}>
        <form onSubmit={form.handleSubmit((data) => updateProfile.mutate(data))}>
          <div className="grid grid-cols-2 gap-3">
            <FormField
              control={form.control}
              name="name"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('profile.name')}</FormLabel>
                  <FormControl>
                    <Input {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="surname"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('profile.surname')}</FormLabel>
                  <FormControl>
                    <Input {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
          </div>
          <FormField
            control={form.control}
            name="email"
            render={({ field }) => (
              <FormItem className="mt-3">
                <FormLabel>{t('profile.email')}</FormLabel>
                <FormControl>
                  <Input type="email" {...field} />
                </FormControl>
                <FormMessage />
              </FormItem>
            )}
          />
          <div className="mt-4">
            <Button type="submit" size="sm" loading={updateProfile.isPending}>
              {t('profile.save')}
            </Button>
          </div>
        </form>
      </Form>
    </SettingsCard>
  );
}
