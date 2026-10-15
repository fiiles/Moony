/**
 * Which picked files the photo gallery uploads. The backend decodes JPEG, PNG, GIF, WebP and BMP
 * (the `image` crate). HEIC is not supported: its in-browser conversion never worked under the
 * app's CSP, so HEIC files are set apart to get their own message.
 */

interface PickedFile {
  name: string;
  type: string;
}

const PHOTO_TYPES = ['image/jpeg', 'image/png', 'image/gif', 'image/webp', 'image/bmp'];

/** The file picker's `accept` attribute. */
export const PHOTO_ACCEPT = PHOTO_TYPES.join(',');

/** Some pickers give files no type; then the extension decides. */
const PHOTO_EXTENSION = /\.(jpe?g|png|gif|webp|bmp)$/i;
const HEIC_TYPE = /^image\/hei[cf]$/;
const HEIC_EXTENSION = /\.hei[cf]$/i;

export function splitPhotoFiles<T extends PickedFile>(
  files: T[]
): { photos: T[]; heic: T[]; unsupported: T[] } {
  const photos: T[] = [];
  const heic: T[] = [];
  const unsupported: T[] = [];
  for (const file of files) {
    if (HEIC_TYPE.test(file.type) || HEIC_EXTENSION.test(file.name)) heic.push(file);
    else if (PHOTO_TYPES.includes(file.type) || (!file.type && PHOTO_EXTENSION.test(file.name)))
      photos.push(file);
    else unsupported.push(file);
  }
  return { photos, heic, unsupported };
}
