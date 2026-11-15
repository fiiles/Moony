import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import { useFormat } from '@/lib/use-format';
import { Sheet } from '@/mobile/components/Sheet';
import { FOOTER_GHOST, FOOTER_PRIMARY } from '@/mobile/components/import/styles';
import type { GalleryPhoto } from '@/mobile/lib/property-detail';
import { FetchingNotice, UnavailableNotice } from '@/mobile/components/documents/AttachmentStates';
import { useAttachmentFetch } from '@/mobile/hooks/use-attachment-fetch';
import { attachmentUrl } from '@/utils/attachment-url';
import { thumbnailShown } from '@/utils/attachment-content';

interface PhotoViewerSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Set together with `open`; the sheet keeps the last one while it slides away. */
  photo: GalleryPhoto | null;
  /** Smazat fotku: the gallery asks in its confirm sheet, over this one, and deletes. */
  onDelete: () => void;
  /** A photo is still being uploaded: Smazat fotku waits (the batch it would delete may be growing). */
  deleteDisabled?: boolean;
  /** When the list the photo comes from was last refreshed: a failed load gets one new attempt per refresh. */
  refreshedAt?: number;
}

// `fetching`: the full photo is not on the phone yet; nothing is mounted that would load it
type Status = 'loading' | 'ready' | 'failed' | 'fetching';

/**
 * The photo viewer (spec §8.8 "Galerie", README §13 "Photo viewer"): a tall sheet filled by one
 * picture, the batch's day as the title and its note under it, Smazat fotku and Zavřít. The photo
 * is shown whole (contained), as the document viewer shows an image, and it comes decrypted through
 * the `moony-file` scheme only while the app is unlocked, nothing written to disk. It cannot be
 * selected, dragged or long-pressed into the system's Copy / Share / Save menu (`-webkit-touch-
 * callout`, no context menu, the picture takes no pointer events), and the box takes panning only
 * (`touch-action: pan-x pan-y`): a pinch over it would zoom the whole page with the sheet. Smazat
 * fotku does not delete: it asks the gallery to confirm, which deletes the photo — and its batch
 * when it was the batch's last — and closes both sheets.
 */
export function PhotoViewerSheet({
  open,
  onOpenChange,
  photo,
  onDelete,
  deleteDisabled = false,
  refreshedAt,
}: PhotoViewerSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const [shown, setShown] = useState(photo);
  const [status, setStatus] = useState<Status>('loading');
  const [wasOpen, setWasOpen] = useState(open);

  // The thumbnail's bytes failed to load although the row says they are here
  const [thumbBroken, setThumbBroken] = useState(false);
  const restart = () => {
    setStatus('loading');
    setThumbBroken(false);
  };

  // A new photo, and every opening, loads again; a refreshed row of the same photo does not
  if (photo !== null && photo !== shown) {
    const another = photo.id !== shown?.id;
    setShown(photo);
    if (another) restart();
  }
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) restart();
  }
  // A load that failed gets one new attempt per refresh of the list
  const [seenRefresh, setSeenRefresh] = useState(refreshedAt);
  if (seenRefresh !== refreshedAt) {
    setSeenRefresh(refreshedAt);
    setThumbBroken(false);
    if (status === 'failed') setStatus('loading');
  }

  const { plan, retrying, tryAgain } = useAttachmentFetch({
    open,
    kind: 'photo',
    id: shown?.id,
    content: shown?.content,
  });
  // The status follows the plan: a spinner while the photo comes, a fresh load once it is here
  const planned: Status =
    plan.stage === 'fetching'
      ? 'fetching'
      : status === 'fetching' || plan.stage === 'unavailable'
        ? 'loading'
        : status;
  if (planned !== status) setStatus(planned);

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      full
      fill
      title={shown ? fmt.day(shown.day, { day: 'numeric', month: 'long', year: 'numeric' }) : ''}
      description={shown?.description || undefined}
      footer={
        <>
          <Button
            variant="ghost"
            className={cn(FOOTER_GHOST, 'text-loss')}
            disabled={deleteDisabled}
            onClick={onDelete}
          >
            {t('property.deletePhoto')}
          </Button>
          <Button className={FOOTER_PRIMARY} onClick={() => onOpenChange(false)}>
            {tc('buttons.close')}
          </Button>
        </>
      }
    >
      {shown && (
        // A column that fills the sheet's body: the box takes all of it
        <div className="relative flex min-h-0 flex-1 flex-col">
          {status === 'loading' && (
            <>
              <Skeleton className="absolute inset-0 z-[1] rounded-r3" />
              <span role="status" className="sr-only">
                {tc('attachments.viewer.loading')}
              </span>
            </>
          )}
          <div
            // No text selection, no callout and no context menu on what is shown; a pan scrolls
            // nothing, a pinch does nothing
            className="relative flex min-h-0 flex-1 select-none items-center justify-center overflow-hidden overscroll-contain rounded-r3 bg-well p-2 [-webkit-touch-callout:none] [touch-action:pan-x_pan-y]"
            onContextMenu={(event) => event.preventDefault()}
          >
            {plan.stage !== 'show' ? (
              <>
                {thumbnailShown(shown.thumbContent) && !thumbBroken ? (
                  <img
                    src={attachmentUrl('photo-thumb', shown.id)}
                    alt=""
                    draggable={false}
                    onError={() => setThumbBroken(true)}
                    className="pointer-events-none block max-h-full max-w-full select-none rounded-r2 object-contain [-webkit-touch-callout:none]"
                  />
                ) : (
                  plan.stage === 'fetching' && (
                    <Skeleton className="absolute inset-0 z-0 rounded-r3" />
                  )
                )}
                {/* The state sits over the thumbnail (or the empty box): the photo itself is not asked for */}
                <div className="absolute inset-x-3 bottom-3 z-[2] rounded-r3 bg-paper p-3 shadow-e1">
                  {plan.stage === 'fetching' ? (
                    <FetchingNotice />
                  ) : (
                    <UnavailableNotice
                      note={plan.note}
                      retry={plan.retry}
                      busy={retrying}
                      onTryAgain={tryAgain}
                    />
                  )}
                </div>
              </>
            ) : status === 'failed' ? (
              <p role="alert" className="m-0 px-1 py-6 text-center text-m-body text-ink-3">
                {t('property.photoFailed')}
              </p>
            ) : (
              <img
                key={shown.id}
                src={attachmentUrl('photo', shown.id)}
                alt=""
                draggable={false}
                onLoad={() => setStatus('ready')}
                onError={() => setStatus('failed')}
                className="pointer-events-none block max-h-full max-w-full select-none rounded-r2 object-contain [-webkit-touch-callout:none]"
              />
            )}
          </div>
        </div>
      )}
    </Sheet>
  );
}
