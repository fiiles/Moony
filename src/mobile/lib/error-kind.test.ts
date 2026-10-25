import { describe, expect, it } from 'vitest';
import { ApiError } from '@/lib/api-error';
import { errorKind } from './error-kind';

const PATH = '/var/mobile/Containers/Data/Documents/Inbox/Vypis_Novak_2026-10.csv';

function apiError(kind: ApiError['kind'], raw: string, key?: string): ApiError {
  return new ApiError({ kind, key, message: 'Něco se nepovedlo', raw });
}

describe('errorKind', () => {
  it('names a backend error by its kind and its i18n key', () => {
    expect(
      errorKind(
        apiError(
          'validation',
          'Validation error: validation.attachmentTooLarge',
          'validation.attachmentTooLarge'
        )
      )
    ).toBe('ApiError(validation: validation.attachmentTooLarge)');
    expect(errorKind(apiError('internal', `Internal error: cannot open ${PATH}`))).toBe(
      'ApiError(internal)'
    );
  });

  it('names any other error by its name, never its message', () => {
    expect(errorKind(new TypeError(`cannot read ${PATH}`))).toBe('TypeError');
    expect(errorKind(new Error(`cannot read ${PATH}`))).toBe('Error');
    const domError = new Error('The file could not be read');
    domError.name = 'NotReadableError';
    expect(errorKind(domError)).toBe('NotReadableError');
  });

  it('names a thrown value that is not an error by its type only', () => {
    expect(errorKind(`Internal error: ${PATH}`)).toBe('string');
    expect(errorKind({ path: PATH })).toBe('object');
    expect(errorKind(null)).toBe('null');
    expect(errorKind(undefined)).toBe('undefined');
  });

  it('never carries a path or a file name', () => {
    for (const error of [
      apiError('internal', `Internal error: ${PATH}`),
      new Error(PATH),
      PATH,
      { message: PATH },
    ]) {
      expect(errorKind(error)).not.toMatch(/Novak|Inbox|\.csv/);
    }
  });
});
