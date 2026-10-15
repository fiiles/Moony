import { describe, expect, it } from 'vitest';
import { PHOTO_ACCEPT, splitPhotoFiles } from './photo-files';

const file = (name: string, type: string) => ({ name, type });

describe('splitPhotoFiles', () => {
  it('accepts the formats the backend decodes', () => {
    const picked = [
      file('a.jpg', 'image/jpeg'),
      file('b.png', 'image/png'),
      file('c.gif', 'image/gif'),
      file('d.webp', 'image/webp'),
      file('e.bmp', 'image/bmp'),
    ];
    expect(splitPhotoFiles(picked)).toEqual({ photos: picked, heic: [], unsupported: [] });
  });

  it('accepts a supported extension when the picker gives no type', () => {
    const jpg = file('IMG_0001.JPEG', '');
    expect(splitPhotoFiles([jpg]).photos).toEqual([jpg]);
  });

  it('sets HEIC apart, by type or by extension', () => {
    const typed = file('IMG_0002.HEIC', 'image/heic');
    const untyped = file('IMG_0003.heif', '');
    expect(splitPhotoFiles([typed, untyped])).toEqual({
      photos: [],
      heic: [typed, untyped],
      unsupported: [],
    });
  });

  it('refuses everything else', () => {
    const pdf = file('scan.pdf', 'application/pdf');
    const tiff = file('scan.tiff', 'image/tiff');
    const unknown = file('notes', '');
    expect(splitPhotoFiles([pdf, tiff, unknown])).toEqual({
      photos: [],
      heic: [],
      unsupported: [pdf, tiff, unknown],
    });
  });

  it('the file picker offers exactly the accepted types', () => {
    expect(PHOTO_ACCEPT.split(',')).toEqual([
      'image/jpeg',
      'image/png',
      'image/gif',
      'image/webp',
      'image/bmp',
    ]);
  });
});
