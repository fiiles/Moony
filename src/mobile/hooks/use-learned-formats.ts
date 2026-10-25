import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { LearnedCsvFormat } from '@shared/schema';
import { learnedFormatsApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { LEARNED_FORMATS_KEY } from '@/mobile/lib/learned-format';

/**
 * The CSV layouts the person confirmed once (spec §8.6 rung 4, `learnedFormatsApi`) for
 * Nastavení → Import: the list, the one used last first, and the two things the page lets them do
 * — rename one and delete one. Both refresh the list; a refusal (an empty or too long name, a format
 * that is gone) is said in a toast and the call rejects, so a sheet keeps its place. The import
 * flow keeps its own copy of what a learned format read; nothing here touches an import.
 */
export function useLearnedFormats() {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();

  const list = useQuery({ queryKey: LEARNED_FORMATS_KEY, queryFn: learnedFormatsApi.list });

  const failed = (error: Error) =>
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });

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

  return { formats: list.data ?? [], isPending: list.isPending, rename, remove };
}
