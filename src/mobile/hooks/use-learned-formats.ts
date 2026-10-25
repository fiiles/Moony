import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { LearnedCsvFormat } from '@shared/schema';
import { learnedFormatsApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { LEARNED_FORMATS_KEY } from '@/mobile/lib/learned-format';

/**
 * The CSV layouts the person confirmed once (spec §8.6 rung 4, `learnedFormatsApi`) for
 * Nastavení → Import: the list, the one used last first, whether it could not be read, and the two
 * things the page lets them do — rename one and delete one. Both refresh the list, whatever their
 * outcome; a refusal (an empty or too long name, a format that is gone) is said in a toast and the
 * mutation errors, so a caller's `onSuccess` (the sheet closing) does not run and the sheet keeps
 * its place — a format that is gone then leaves the list, which closes its sheet. The import flow
 * keeps its own copy of what a learned format read; nothing here touches an import.
 */
export function useLearnedFormats() {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();

  const list = useQuery({ queryKey: LEARNED_FORMATS_KEY, queryFn: learnedFormatsApi.list });

  // A refusal is said, and the list is read again: a format that is gone (NotFound) leaves it
  const failed = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    void queryClient.invalidateQueries({ queryKey: LEARNED_FORMATS_KEY });
  };

  const rename = useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) => learnedFormatsApi.rename(id, name),
    onSuccess: (saved) => {
      // The renamed row at once: the sheet that asked is still open while the list is fetched again
      queryClient.setQueryData<LearnedCsvFormat[]>(LEARNED_FORMATS_KEY, (current) =>
        current?.map((format) => (format.id === saved.id ? saved : format))
      );
      void queryClient.invalidateQueries({ queryKey: LEARNED_FORMATS_KEY });
      toast(t('settings.import.renamed'));
    },
    onError: failed,
  });

  const remove = useMutation({
    mutationFn: (id: string) => learnedFormatsApi.remove(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: LEARNED_FORMATS_KEY });
      toast(t('settings.import.deleted'));
    },
    onError: failed,
  });

  return {
    formats: list.data ?? [],
    isPending: list.isPending,
    isError: list.isError,
    rename,
    remove,
  };
}
