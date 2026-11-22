import { describe, expect, it } from 'vitest';
import { pdfFailure } from './pdf-failure';

/** pdf.js's own shape: a `BaseException` named `PasswordException` with the response code. */
function passwordException(code: number): Error {
  const error = new Error('No password given');
  error.name = 'PasswordException';
  return Object.assign(error, { code });
}

describe('pdfFailure', () => {
  it('tells a document that needs a password from one whose password did not match', () => {
    expect(pdfFailure(passwordException(1))).toBe('needsPassword');
    expect(pdfFailure(passwordException(2))).toBe('wrongPassword');
  });

  it('treats every other failure as a broken preview', () => {
    expect(pdfFailure(new Error('HTTP 404'))).toBe('failed');
    expect(pdfFailure(Object.assign(new Error('x'), { name: 'InvalidPDFException' }))).toBe(
      'failed'
    );
    // A password exception with a code pdf.js does not define is not a password question
    expect(pdfFailure(passwordException(7))).toBe('failed');
    expect(pdfFailure('boom')).toBe('failed');
    expect(pdfFailure(null)).toBe('failed');
  });
});
