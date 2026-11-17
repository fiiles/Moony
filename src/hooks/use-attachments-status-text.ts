import { useTranslation } from 'react-i18next';
import type { SyncAttachments } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { fileSizeText } from '@/utils/file-size';
import {
  failedUploadLine,
  shownProblem,
  waitingUploadLine,
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
 * The lines sync adds about documents and photos, shared by every surface, each `null` when there
 * is nothing to say (they are data like any other: no row of their own, the one sync status covers
 * them): `waiting` what waits to upload, with its size; `failedUpload` uploads that cannot go by
 * themselves; `problem` what keeps them from moving while the rest of sync goes on (`shownProblem`).
 */
export function useAttachmentsStatusText(status: SyncAttachments | undefined): {
  waiting: string | null;
  problem: string | null;
  failedUpload: string | null;
} {
  const { t } = useTranslation('deviceSync');
  const fmt = useFormat();
  const a = status ?? NONE;
  const line = waitingUploadLine(a);
  const problem = shownProblem(a);
  const stuck = failedUploadLine(a);
  return {
    waiting:
      line === null
        ? null
        : withoutDanglingSize(
            t('attachments.waiting', {
              count: line.count,
              size: line.bytes > 0 ? fileSizeText(line.bytes, fmt) : '',
            })
          ),
    problem: problem ? t(`attachments.problem.${problem}`) : null,
    failedUpload: stuck === null ? null : t('attachments.failedUpload', { count: stuck }),
  };
}
