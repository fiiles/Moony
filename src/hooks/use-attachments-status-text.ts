import { useTranslation } from 'react-i18next';
import type { SyncAttachments } from '@shared/schema';
import { formatFileSize } from '@/utils/file-size';
import { attachmentsLine } from '@/utils/sync-attachments';

/** Before the status has answered: nothing waits, nothing is wrong. */
const NONE: SyncAttachments = {
  waitingUpload: 0,
  waitingUploadBytes: 0,
  notHere: 0,
  notOnServer: 0,
  failed: 0,
  notOnDevice: 0,
  problem: null,
};

/**
 * The words of the "Documents and photos" status row, shared by every surface that shows it:
 * `value` is "All uploaded" or what waits (with its size); `problem` the line of a problem that
 * keeps documents and photos from moving (the rest of sync goes on), whatever waits.
 */
export function useAttachmentsStatusText(status: SyncAttachments | undefined): {
  value: string;
  problem: string | null;
} {
  const { t } = useTranslation('deviceSync');
  const a = status ?? NONE;
  const line = attachmentsLine(a);
  // A problem line only exists while something waits: the row still names what waits
  const value =
    line.kind === 'allUploaded'
      ? t('attachments.allUploaded')
      : t('attachments.waiting', {
          count: a.waitingUpload,
          size: formatFileSize(a.waitingUploadBytes),
        }).replace(/ · $/, '');
  return { value, problem: a.problem ? t(`attachments.problem.${a.problem}`) : null };
}
