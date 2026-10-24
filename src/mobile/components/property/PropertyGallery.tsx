import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { RealEstatePhotoBatch } from '@shared/schema';
import { realEstateApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { attachmentUrl } from '@/utils/attachment-url';
import { PHOTO_ACCEPT, splitPhotoFiles } from '@/utils/photo-files';
import { CaptureButtons } from '@/mobile/components/documents/CaptureButtons';
import { SectionHead } from '@/mobile/components/SectionHead';
import { usePropertyPhotoMutations } from '@/mobile/hooks/use-property-mutations';
import { galleryPhotos, type GalleryPhoto } from '@/mobile/lib/property-detail';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { PhotoViewerSheet } from '@/mobile/sheets/PhotoViewerSheet';

/** Stable empty list: a new `[]` per render would keep the memo below from ever settling. */
const NO_BATCHES: RealEstatePhotoBatch[] = [];

interface PropertyGalleryProps {
  propertyId: string;
}

/**
 * "Galerie" of a property (spec §8.8 "Nemovitost"; the desktop's photo timeline): the head with the
 * count of photos, the photos as a grid of three square thumbnails, the newest batch first, and
 * Vyfotit · Vybrat z fotek under it. A photo from the camera or the library goes into today's
 * batch — the day's photos stay together (`uploadToTodayBatch`) — one by one, the buttons saying
 * "Nahrávám 2 / 5" while it runs. HEIC (not supported) and other formats the backend cannot decode
 * are turned away by name before anything is read (the desktop's `gallery.errors.*`), the rest are
 * uploaded; a cancelled capture uploads nothing. A tap on a thumbnail opens the viewer (the whole
 * photo, its day and note, Smazat fotku, which asks first). The list is the desktop's
 * (`['real-estate-photo-batches', id]`). Thumbnails and photos come decrypted through the
 * `moony-file` scheme and are never offered to the system's Copy / Share / Save menu.
 */
export function PropertyGallery({ propertyId }: PropertyGalleryProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { upload, progress, removePhoto } = usePropertyPhotoMutations(propertyId);

  const {
    data: batches = NO_BATCHES,
    isLoading,
    error: loadError,
    refetch,
  } = useQuery<RealEstatePhotoBatch[]>({
    queryKey: ['real-estate-photo-batches', propertyId],
    queryFn: () => realEstateApi.getPhotoBatches(propertyId),
  });
  const photos = useMemo(() => galleryPhotos(batches), [batches]);

  // The sheets keep their photo while they slide away: which one and whether it is open are apart
  const [viewing, setViewing] = useState<GalleryPhoto | null>(null);
  const [viewOpen, setViewOpen] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);

  // What the confirm sentence says is read from the batches as they are now (refreshed when the
  // sheet opens): an upload may have landed since the thumbnail was tapped. The delete itself decides
  // on a fresh read of its own as well (`deletePhotoLive`).
  const live = viewing ? (photos.find((photo) => photo.id === viewing.id) ?? viewing) : null;

  const chosen = (files: File[]) => {
    const { photos: usable, heic, unsupported } = splitPhotoFiles(files);
    const problems = [
      ...heic.map((file) => tr('gallery.errors.heicUnsupported', { name: file.name })),
      ...unsupported.map((file) => tr('gallery.errors.unsupportedFormat', { name: file.name })),
    ];
    if (problems.length > 0) {
      toast.error(tr('gallery.invalidFiles'), { description: problems.join('\n') });
    }
    if (usable.length > 0) upload.mutate(usable);
  };

  // Between the pick and the first photo the backend is asked for today's batch: that is photo 1
  const uploading = upload.isPending
    ? t('property.uploading', {
        current: progress?.current ?? 1,
        total: progress?.total ?? upload.variables?.length ?? 1,
      })
    : null;

  return (
    <>
      <SectionHead
        title={tr('detail.tabs.gallery')}
        note={photos.length > 0 ? t('property.photoCount', { count: photos.length }) : undefined}
      />
      {loadError ? (
        <p role="alert" className="mx-0.5 mb-3.5 text-m-sub text-ink-3">
          {translateApiError(loadError, tc)}
        </p>
      ) : isLoading ? (
        <p role="status" className="mx-0.5 mb-3.5 text-m-sub text-ink-3">
          {tc('status.loading')}
        </p>
      ) : photos.length === 0 ? (
        <p className="mx-0.5 mb-3.5 text-m-sub text-ink-3">{tr('gallery.noPhotos')}</p>
      ) : (
        <ul className="m-0 mb-3.5 grid list-none grid-cols-3 gap-1.5 p-0">
          {photos.map((photo) => (
            <li key={photo.id}>
              <button
                type="button"
                aria-label={t('property.photoOpen', { day: fmt.day(photo.day) })}
                onClick={() => {
                  setViewing(photo);
                  setViewOpen(true);
                }}
                // No text selection and no callout: a long press must not offer Save or Copy
                className="relative block aspect-square w-full select-none overflow-hidden rounded-r2 border border-line bg-well p-0 [-webkit-touch-callout:none] active:opacity-80"
              >
                <img
                  src={attachmentUrl('photo-thumb', photo.id)}
                  alt=""
                  loading="lazy"
                  draggable={false}
                  className="pointer-events-none block size-full select-none object-cover [-webkit-touch-callout:none]"
                />
              </button>
            </li>
          ))}
        </ul>
      )}
      <CaptureButtons
        cameraLabel={uploading ?? t('documents.takePhoto')}
        pickLabel={uploading ?? t('documents.fromPhotos')}
        pickAccept={PHOTO_ACCEPT}
        multiple
        disabled={upload.isPending || removePhoto.isPending}
        onFiles={chosen}
      />
      {uploading && (
        <span role="status" className="sr-only">
          {uploading}
        </span>
      )}
      <PhotoViewerSheet
        open={viewOpen}
        onOpenChange={setViewOpen}
        photo={viewing}
        onDelete={() => {
          setConfirmOpen(true);
          // The sentence in the confirm sheet follows the batches as they are now
          void refetch();
        }}
      />
      <ConfirmSheet
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={t('property.deletePhotoConfirm.title')}
        description={
          live?.last
            ? t('property.deletePhotoConfirm.descriptionLast', { day: fmt.day(live.day) })
            : t('property.deletePhotoConfirm.description')
        }
        confirmLabel={t('property.deletePhoto')}
        isPending={removePhoto.isPending}
        onConfirm={() => {
          if (!viewing) return;
          removePhoto.mutate(viewing.id, {
            onSuccess: () => {
              setConfirmOpen(false);
              setViewOpen(false);
            },
          });
        }}
      />
    </>
  );
}
