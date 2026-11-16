import { useTranslation } from 'react-i18next';
import type { SyncAttachments } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { fileSizeText } from '@/utils/file-size';
import {
  attachmentsLine,
  failedUploadLine,
  shownProblem,
  withoutDanglingSize,
} from '@/utils/sync-attachments';

/** Before the status has answered: nothing waits, nothing is wrong. */
const NONE: SyncAttachments = {
  waitingUpload: 0,
  waitingUploadBytes: 0,
  notHere: 0,
  notOnServer: 0,
  failed: 0,
  notOnDevice: 0,
  failedUpload: 0,
  problem: null,
};

/**
 * The words of the "Documents and photos" status row, shared by every surface that shows it:
 * `value` is "All uploaded" or what waits (with its size), `null` while only a problem is shown;
 * `problem` the line of the problem that keeps documents and photos from moving (the rest of sync
 * goes on), `null` when there is none to tell (`shownProblem`); `failedUpload` the line about uploads
 * that cannot go by themselves, shown under the value, `null` when none.
 */
export function useAttachmentsStatusText(status: SyncAttachments | undefined): {
  value: string | null;
  problem: string | null;
  failedUpload: string | null;
} {
  const { t } = useTranslation('deviceSync');
  const fmt = useFormat();
  const a = status ?? NONE;
  const line = attachmentsLine(a);
  const problem = shownProblem(a);
  let value: string | null = null;
  if (line.kind === 'allUploaded') value = t('attachments.allUploaded');
  if (line.kind === 'waiting') {
    value = withoutDanglingSize(
      t('attachments.waiting', {
        count: line.count,
        size: line.bytes > 0 ? fileSizeText(line.bytes, fmt) : '',
      })
    );
  }
  const stuck = failedUploadLine(a);
  return {
    value,
    problem: problem ? t(`attachments.problem.${problem}`) : null,
    failedUpload: stuck === null ? null : t('attachments.failedUpload', { count: stuck }),
  };
}
