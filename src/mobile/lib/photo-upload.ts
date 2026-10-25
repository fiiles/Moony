import type { RealEstatePhoto, RealEstatePhotoBatch } from '@shared/schema';
import { utcDayFloor } from '@/utils/chart-axis';
import { errorKind } from './error-kind';
import { photoDeletion, todayBatch } from './property-detail';

/** The backend commands the upload needs (`realEstateApi`'s); injected so the rules can be tested. */
export interface PhotoApi {
  getPhotoBatches(realEstateId: string): Promise<RealEstatePhotoBatch[]>;
  createPhotoBatch(
    realEstateId: string,
    data: { photoDate: number }
  ): Promise<RealEstatePhotoBatch>;
  addPhotoToBatch(batchId: string, bytes: Uint8Array): Promise<RealEstatePhoto>;
  deletePhotoBatch(batchId: string): Promise<void>;
}

/**
 * What the upload reads of a picked file (a `File`): its size and its bytes. Not its name: a
 * photographed contract's name says whose it is, and nothing here may log it.
 */
export interface PhotoSource {
  size: number;
  arrayBuffer(): Promise<ArrayBuffer>;
}

/** The photo being sent: 1-based, so "Nahrávám 2 / 5" says which one is on its way. */
export interface UploadProgress {
  current: number;
  total: number;
}

export interface UploadResult {
  /** The batch the photos went into. */
  batchId: string;
  saved: number;
  /** Photos that could not be read or that the backend refused. */
  skipped: number;
}

/**
 * Sends picked photos into **today's batch** of a property (spec §8.8 "Galerie": the photos of one
 * day go into that day's batch), the way the desktop's gallery sends a batch's files, one by one:
 * each file's bytes go straight to the backend (`addPhotoToBatch`), so no plaintext copy is written
 * anywhere, and a photo that fails is skipped. The batches are read from the backend, not from a
 * cache that a moment-old upload may not have reached yet, or two photos taken one after the other
 * would each start a batch of the day.
 *
 * Today's batch is made when it is missing, dated today's UTC day. When not one photo was saved the
 * upload fails with the first error — and a batch made for it is deleted again, so a failed upload
 * leaves no empty batch (a batch that was already there is never touched).
 */
export async function uploadToTodayBatch(
  api: PhotoApi,
  realEstateId: string,
  files: readonly PhotoSource[],
  today: number,
  onProgress?: (progress: UploadProgress) => void
): Promise<UploadResult> {
  if (files.length === 0) throw new Error('No photos to upload');

  const batches = await api.getPhotoBatches(realEstateId);
  const existing = todayBatch(batches, today);
  const batch =
    existing ?? (await api.createPhotoBatch(realEstateId, { photoDate: utcDayFloor(today) }));

  let saved = 0;
  let failure: unknown = null;
  for (const [index, file] of files.entries()) {
    onProgress?.({ current: index + 1, total: files.length });
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      await api.addPhotoToBatch(batch.id, bytes);
      saved += 1;
    } catch (error) {
      // Which photo, how big and what kind of failure: never its name, nor the error's text
      console.error(
        `Photo ${index + 1} of ${files.length} (${file.size} bytes) could not be saved: ${errorKind(error)}`
      );
      failure ??= error;
    }
  }

  if (saved === 0) {
    if (existing === null) {
      try {
        await api.deletePhotoBatch(batch.id);
      } catch (error) {
        console.error(`The empty photo batch could not be removed: ${errorKind(error)}`);
      }
    }
    throw failure;
  }
  return { batchId: batch.id, saved, skipped: files.length - saved };
}

/** The backend commands the delete needs (`realEstateApi`'s); injected so the rules can be tested. */
export interface PhotoDeleteApi {
  getPhotoBatches(realEstateId: string): Promise<RealEstatePhotoBatch[]>;
  deletePhoto(photoId: string): Promise<void>;
  deletePhotoBatch(batchId: string): Promise<void>;
}

/**
 * Deletes a photo, deciding what goes with it from the batches **as they are when the delete is
 * confirmed** (`photoDeletion`), never from what they were when the photo was opened: an upload into
 * today's batch can land in between, and the batch is deleted only if the photo is still its only
 * one — otherwise it would take the new photos with it. A batch's last photo goes with the batch in
 * the one transaction of `deletePhotoBatch`, so none is left empty. Resolves with what was deleted;
 * `gone` when the photo was deleted elsewhere already.
 */
export async function deletePhotoLive(
  api: PhotoDeleteApi,
  realEstateId: string,
  photoId: string
): Promise<'batch' | 'photo' | 'gone'> {
  const deletion = photoDeletion(await api.getPhotoBatches(realEstateId), photoId);
  if (deletion.kind === 'batch') await api.deletePhotoBatch(deletion.batchId);
  else if (deletion.kind === 'photo') await api.deletePhoto(deletion.photoId);
  return deletion.kind;
}
