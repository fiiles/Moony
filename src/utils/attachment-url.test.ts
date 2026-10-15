import { describe, expect, it } from 'vitest';
import { attachmentPath } from './attachment-url';

describe('attachmentPath', () => {
  it('names the route and the row id', () => {
    expect(attachmentPath('photo', 'abc-1')).toBe('photo/abc-1');
    expect(attachmentPath('photo-thumb', 'abc-1')).toBe('photo-thumb/abc-1');
    expect(attachmentPath('real-estate-document', 'd')).toBe('real-estate-document/d');
    expect(attachmentPath('insurance-document', 'd')).toBe('insurance-document/d');
  });
});
