import { describe, expect, it } from 'vitest';
import { getFormatters } from '@/lib/format';
import {
  MAX_DOCUMENT_BYTES,
  documentNameFrom,
  documentSub,
  fileSizeText,
  pickedFileProblem,
} from './documents';

const cs = getFormatters('cs-CZ', 'CZK');
const en = getFormatters('en-US', 'USD');

const file = (name: string, type: string, size = 1234) => ({ name, type, size });

describe('pickedFileProblem', () => {
  it('accepts a pdf, the photo types and an office document', () => {
    expect(pickedFileProblem(file('Smlouva.pdf', 'application/pdf'))).toBeNull();
    expect(pickedFileProblem(file('scan.jpg', 'image/jpeg'))).toBeNull();
    expect(pickedFileProblem(file('scan.png', 'image/png'))).toBeNull();
    expect(pickedFileProblem(file('scan.webp', 'image/webp'))).toBeNull();
    expect(
      pickedFileProblem(
        file(
          'Smlouva.docx',
          'application/vnd.openxmlformats-officedocument.wordprocessingml.document'
        )
      )
    ).toBeNull();
  });

  it('accepts a file with no type or no extension (the viewer says what it cannot show)', () => {
    expect(pickedFileProblem(file('smlouva', ''))).toBeNull();
    expect(pickedFileProblem(file('smlouva.PDF', ''))).toBeNull();
  });

  it('refuses a HEIC photo by its type, by its name, and in any case', () => {
    expect(pickedFileProblem(file('IMG_1.heic', 'image/heic'))).toBe('heic');
    expect(pickedFileProblem(file('IMG_1.heif', 'image/heif'))).toBe('heic');
    expect(pickedFileProblem(file('IMG_2.HEIC', ''))).toBe('heic');
    expect(pickedFileProblem(file('IMG_2.Heif', ''))).toBe('heic');
    // The type alone is enough, whatever the file is called
    expect(pickedFileProblem(file('image.jpg', 'image/heic'))).toBe('heic');
  });

  it('refuses an empty file', () => {
    expect(pickedFileProblem(file('Smlouva.pdf', 'application/pdf', 0))).toBe('empty');
  });

  it('refuses a file over 100 MB and takes one of exactly 100 MB', () => {
    expect(MAX_DOCUMENT_BYTES).toBe(104_857_600);
    expect(pickedFileProblem(file('a.pdf', 'application/pdf', MAX_DOCUMENT_BYTES))).toBeNull();
    expect(pickedFileProblem(file('a.pdf', 'application/pdf', MAX_DOCUMENT_BYTES + 1))).toBe(
      'tooLarge'
    );
  });

  it('names a HEIC photo before an empty one: the format is what the person can fix', () => {
    expect(pickedFileProblem(file('IMG.HEIC', 'image/heic', 0))).toBe('heic');
  });
});

describe('documentNameFrom', () => {
  const PHOTO = 'Foto 24. 10. 2026';

  it('takes a picked file’s name without its extension', () => {
    expect(documentNameFrom('Smlouva 2024.pdf', 'picker', PHOTO)).toBe('Smlouva 2024');
    expect(documentNameFrom('IMG_0042.JPG', 'picker', PHOTO)).toBe('IMG_0042');
  });

  it('cuts only the last extension and keeps a name without one', () => {
    expect(documentNameFrom('archiv.tar.gz', 'picker', PHOTO)).toBe('archiv.tar');
    expect(documentNameFrom('smlouva', 'picker', PHOTO)).toBe('smlouva');
    expect(documentNameFrom('Dodatek č. 2', 'picker', PHOTO)).toBe('Dodatek č. 2');
  });

  it('uses the dated photo name for a capture, whatever the camera called the file', () => {
    expect(documentNameFrom('image.jpg', 'camera', PHOTO)).toBe(PHOTO);
  });

  it('falls back to the photo name when nothing is left of a picked name', () => {
    expect(documentNameFrom('.pdf', 'picker', PHOTO)).toBe(PHOTO);
    expect(documentNameFrom('   .pdf', 'picker', PHOTO)).toBe(PHOTO);
    expect(documentNameFrom('', 'picker', PHOTO)).toBe(PHOTO);
  });

  it('trims the name to 200 characters', () => {
    const long = 'x'.repeat(250);
    expect(documentNameFrom(`${long}.pdf`, 'picker', PHOTO)).toBe('x'.repeat(200));
    expect(documentNameFrom('a.pdf', 'camera', 'y'.repeat(300))).toBe('y'.repeat(200));
  });

  it('keeps a name within the backend’s 200 bytes, never cutting a character in two', () => {
    // 'ž' is two bytes, a Japanese character three, an emoji four
    expect(documentNameFrom(`${'ž'.repeat(150)}.pdf`, 'picker', PHOTO)).toBe('ž'.repeat(100));
    expect(documentNameFrom(`${'書'.repeat(100)}.pdf`, 'picker', PHOTO)).toBe('書'.repeat(66));
    const emoji = '📄'.repeat(60);
    expect([...documentNameFrom(`${emoji}.pdf`, 'picker', PHOTO)]).toHaveLength(50);
  });

  it('trims the white space around a name, also where the cut leaves some', () => {
    expect(documentNameFrom('  Smlouva .pdf', 'picker', PHOTO)).toBe('Smlouva');
    expect(documentNameFrom(`${'x'.repeat(199)} z.pdf`, 'picker', PHOTO)).toBe('x'.repeat(199));
  });
});

describe('fileSizeText', () => {
  it('writes bytes, kilobytes without decimals and megabytes with one', () => {
    expect(fileSizeText(512, cs)).toBe('512 B');
    expect(fileSizeText(830 * 1024, cs)).toBe('830 kB');
    expect(fileSizeText(Math.round(1.2 * 1024 * 1024), cs)).toBe('1,2 MB');
    expect(fileSizeText(Math.round(1.2 * 1024 * 1024), en)).toBe('1.2 MB');
    expect(fileSizeText(100 * 1024 * 1024, cs)).toBe('100 MB');
  });

  it('never writes 1024 kB', () => {
    expect(fileSizeText(1024 * 1024 - 1, cs)).toBe('1 MB');
  });
});

describe('documentSub', () => {
  // Noon UTC: the same calendar day in every time zone the app runs in
  const UPLOADED = Date.UTC(2026, 9, 24, 12) / 1000;
  const label = (type: string) => ({ contract: 'smlouva', other: 'jiné' })[type] ?? type;
  const doc = {
    fileType: 'contract',
    fileSize: Math.round(1.2 * 1024 * 1024),
    uploadedAt: UPLOADED,
  };

  it('joins the type, the size and the upload day', () => {
    expect(documentSub(doc, label, cs)).toBe('smlouva · 1,2 MB · 24. 10. 2026');
    expect(documentSub({ ...doc, fileSize: 830 * 1024, fileType: 'other' }, label, cs)).toBe(
      'jiné · 830 kB · 24. 10. 2026'
    );
  });

  it('leaves the size out when it is unknown', () => {
    expect(documentSub({ ...doc, fileSize: null }, label, cs)).toBe('smlouva · 24. 10. 2026');
  });

  it('asks the caller for the type’s label and keeps a type it does not know', () => {
    expect(documentSub({ ...doc, fileType: 'appraisal' }, label, cs)).toBe(
      'appraisal · 1,2 MB · 24. 10. 2026'
    );
  });
});
