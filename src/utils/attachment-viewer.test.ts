import { describe, expect, it } from 'vitest';
import { defaultFileName, viewerKind } from './attachment-viewer';

describe('viewerKind', () => {
  it('opens PDFs and common images in the app', () => {
    expect(viewerKind('application/pdf')).toBe('pdf');
    expect(viewerKind('image/jpeg')).toBe('image');
    expect(viewerKind('image/png')).toBe('image');
    expect(viewerKind('image/webp')).toBe('image');
  });

  it('sends everything else to another app', () => {
    expect(viewerKind('image/svg+xml')).toBe('external');
    expect(viewerKind('image/heic')).toBe('external');
    expect(viewerKind('text/html')).toBe('external');
    expect(
      viewerKind('application/vnd.openxmlformats-officedocument.wordprocessingml.document')
    ).toBe('external');
    expect(viewerKind('application/octet-stream')).toBe('external');
  });
});

describe('defaultFileName', () => {
  it('adds the original extension once', () => {
    expect(defaultFileName('Kupní smlouva', 'pdf')).toBe('Kupní smlouva.pdf');
    expect(defaultFileName('Scan.PDF', 'pdf')).toBe('Scan.PDF');
    expect(defaultFileName('Poznámky', null)).toBe('Poznámky');
  });

  it('keeps the name usable as a file name', () => {
    expect(defaultFileName('a/b: c?', 'txt')).toBe('a_b_ c_.txt');
    expect(defaultFileName('   ', 'pdf')).toBe('document.pdf');
  });
});
