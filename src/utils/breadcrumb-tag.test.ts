import { describe, expect, it } from 'vitest';
import { breadcrumbTag } from './breadcrumb-tag';

describe('breadcrumbTag', () => {
  it('lower-cases the descriptor', () => {
    expect(breadcrumbTag('Synchronizace', 'cs-CZ')).toBe('synchronizace');
    expect(breadcrumbTag('Bank accounts', 'en-US')).toBe('bank accounts');
  });

  it('keeps German nouns capitalised', () => {
    expect(breadcrumbTag('Synchronisierung', 'de-DE')).toBe('Synchronisierung');
    expect(breadcrumbTag('Bankkonten', 'de')).toBe('Bankkonten');
  });
});
