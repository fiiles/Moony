import { useMemo, useState } from 'react';
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
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Button } from '@/components/ui/button';
import { SettingsCard } from '@/components/settings/SettingsCard';
import { useAuth } from '@/hooks/use-auth';
import { useSettingsMutations } from '@/hooks/use-settings-mutations';
import { useLanguage } from '@/i18n/I18nProvider';
import { RULE_PACK_COUNTRY_IDS, countryOptionsFromPacks } from '@/utils/locale-defaults';

const NONE = '__none__';

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
  const { getLocale } = useLanguage();
  const countryOptions = useMemo(
    () => countryOptionsFromPacks([...RULE_PACK_COUNTRY_IDS], getLocale()),
    [getLocale]
  );
  const [country, setCountry] = useState<string>(user?.country?.toLowerCase() || NONE);

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
        <form
          onSubmit={form.handleSubmit((data) =>
            updateProfile.mutate({
              ...data,
              country: country === NONE ? '' : country.toUpperCase(),
            })
          )}
        >
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
          <div className="mt-3 space-y-1.5">
            <Label htmlFor="profile-country">{t('profile.country')}</Label>
            <Select value={country} onValueChange={setCountry}>
              <SelectTrigger id="profile-country">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NONE}>{t('profile.countryNone')}</SelectItem>
                {countryOptions.map((option) => (
                  <SelectItem key={option.packId} value={option.packId}>
                    {option.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-micro font-500 text-ink-4">{t('profile.countryHint')}</p>
          </div>
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
