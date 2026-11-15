import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import type { TryAgainAction } from '@/mobile/lib/attachment-fetch';
import type { AttachmentNote } from '@/utils/attachment-content';

/** The spinner and its words while a viewer's content comes from the sync server. */
export function FetchingNotice() {
  const { t: tc } = useTranslation('common');
  return (
    <div role="status" className="flex flex-col items-center gap-2 text-center">
      <Loader2 aria-hidden className="size-7 animate-spin text-ink-3" strokeWidth={1.75} />
      <span className="text-m-body text-ink-3">{tc('attachments.content.fetching')}</span>
    </div>
  );
}

interface UnavailableNoticeProps {
  note: AttachmentNote;
  /** Offered only for a failed download: "Try again". */
  retry: TryAgainAction | null;
  busy: boolean;
  onTryAgain: () => void;
}

/** Why a viewer has nothing to show (`failed`, `notOnServer`, `missing`), with Try again for a failed one. */
export function UnavailableNotice({ note, retry, busy, onTryAgain }: UnavailableNoticeProps) {
  const { t: tc } = useTranslation('common');
  return (
    <div className="flex flex-col items-center gap-3 text-center">
      <p role="alert" className="m-0 text-m-body text-ink-3">
        {tc(`attachments.content.${note}`)}
      </p>
      {retry && (
        <Button
          type="button"
          variant="secondary"
          className="h-auto min-h-12 rounded-[10px] px-5 text-m-row"
          loading={busy}
          disabled={busy}
          onClick={onTryAgain}
        >
          {tc('attachments.content.retry')}
        </Button>
      )}
    </div>
  );
}
