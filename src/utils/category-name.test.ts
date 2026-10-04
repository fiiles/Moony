import { describe, expect, it } from 'vitest';
import type { TFunction } from 'i18next';
import { categoryDisplayName, hasSeededName, SYSTEM_CATEGORY_DEFAULT_NAMES } from './category-name';

/** A stand-in for i18next's t() with a tiny Czech dictionary. */
const czech = ((key: string, options?: { defaultValue?: string }) =>
  ({ 'categoryNames.cat_groceries': 'Potraviny', 'categoryNames.cat_other': 'Ostatní' })[key] ??
  options?.defaultValue ??
  key) as unknown as TFunction;

describe('categoryDisplayName', () => {
  it('translates a seeded system category', () => {
    expect(
      categoryDisplayName({ id: 'cat_groceries', name: 'Groceries', isSystem: true }, czech)
    ).toBe('Potraviny');
  });

  it('shows the stored name of a custom category, never a translation', () => {
    expect(categoryDisplayName({ id: 'abc-123', name: 'Pets', isSystem: false }, czech)).toBe(
      'Pets'
    );
    // Even when a custom category happens to share a system id shape.
    expect(categoryDisplayName({ id: 'cat_groceries', name: 'Pets', isSystem: false }, czech)).toBe(
      'Pets'
    );
  });

  it('lets a renamed system category keep the name the user typed', () => {
    expect(categoryDisplayName({ id: 'cat_groceries', name: 'Food', isSystem: true }, czech)).toBe(
      'Food'
    );
  });

  it('falls back to the stored name when a seeded name has no translation', () => {
    expect(categoryDisplayName({ id: 'cat_taxes', name: 'Taxes', isSystem: true }, czech)).toBe(
      'Taxes'
    );
  });
});

describe('hasSeededName', () => {
  it('is true only for system categories that still carry their seeded name', () => {
    expect(hasSeededName({ id: 'cat_other', name: 'Other', isSystem: true })).toBe(true);
    expect(hasSeededName({ id: 'cat_other', name: 'Misc', isSystem: true })).toBe(false);
    expect(hasSeededName({ id: 'x', name: 'Other', isSystem: false })).toBe(false);
  });

  it('knows all 17 seeded system categories', () => {
    expect(Object.keys(SYSTEM_CATEGORY_DEFAULT_NAMES)).toHaveLength(17);
  });
});
