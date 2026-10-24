import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { authApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { PickRow } from '@/mobile/components/PickRow';
import { Sheet } from '@/mobile/components/Sheet';
import { useTheme } from '@/theme/ThemeProvider';
import type { ThemePreference } from '@/utils/theme';

interface AppearanceSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/** The order of the rows: the default first, as in the desktop's Vzhled. */
const CHOICES: readonly ThemePreference[] = ['system', 'light', 'dark'];

/**
 * "Vzhled" (spec §8.9): Systém / Světlý / Tmavý, the current one checked. It saves as the
 * desktop's `AppearanceCard` does: a tap applies the theme at once and writes the profile in the
 * background; a failed save puts the previous theme back and shows the usual error toast, and the
 * sheet stays open. A successful save closes it without a toast (the screen changing is the
 * answer), as does a tap on the current choice. A tap while a save is pending is ignored.
 */
export function AppearanceSheet({ open, onOpenChange }: AppearanceSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const { preference, setPreference } = useTheme();
  const queryClient = useQueryClient();

  const save = useMutation({
    mutationFn: async ({ theme }: { theme: ThemePreference; previous: ThemePreference }) => {
      await authApi.updateProfile({ theme });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['user-profile'] });
    },
    onError: (error: Error, { previous }) => {
      setPreference(previous);
      toast.error(ts('toast.updateFailed'), { description: translateApiError(error, tc) });
    },
  });

  const select = (next: ThemePreference) => {
    if (save.isPending) return;
    if (next === preference) {
      onOpenChange(false);
      return;
    }
    const previous = preference;
    setPreference(next);
    save.mutate({ theme: next, previous }, { onSuccess: () => onOpenChange(false) });
  };

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={ts('appearance.title')}
      description={t('settings.appearance.lead')}
      footer={
        <Button
          variant="ghost"
          className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
          onClick={() => onOpenChange(false)}
        >
          {tc('buttons.cancel')}
        </Button>
      }
    >
      <div
        role="radiogroup"
        aria-label={ts('appearance.title')}
        aria-busy={save.isPending || undefined}
        className="divide-y divide-line-soft"
      >
        {CHOICES.map((choice) => (
          <PickRow
            key={choice}
            label={ts(`appearance.${choice}`)}
            checked={choice === preference}
            onSelect={() => select(choice)}
          />
        ))}
      </div>
    </Sheet>
  );
}
