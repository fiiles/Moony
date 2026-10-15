import { describe, expect, it } from 'vitest';
import {
  foldName,
  matchSourceCategories,
  sourceCategoryMapping,
  type MatchableCategory,
} from './source-categories';

const cats: MatchableCategory[] = [
  { id: 'cat_groceries', name: 'Groceries', parentId: 'cat_food', isSystem: true },
  { id: 'cat_food', name: 'Food', parentId: null, isSystem: true },
  { id: 'u1', name: 'Kino', parentId: null, isSystem: false },
  { id: 'u2', name: 'Dárky', parentId: null, isSystem: false },
  { id: 'u3', name: 'Dárky', parentId: 'cat_food', isSystem: false },
];
const czech: Record<string, string> = { cat_groceries: 'Potraviny', cat_food: 'Jídlo' };
const display = (c: MatchableCategory) => (c.isSystem ? (czech[c.id] ?? c.name) : c.name);

describe('matchSourceCategories', () => {
  it('matches a full path, a leaf, the display name and folds diacritics', () => {
    expect(
      matchSourceCategories(['Food:Groceries', 'potraviny', 'KINO', 'jidlo'], cats, display)
    ).toEqual({
      'Food:Groceries': 'cat_groceries',
      potraviny: 'cat_groceries',
      KINO: 'u1',
      jidlo: 'cat_food',
    });
  });

  it('leaves ambiguous and unknown names unmapped', () => {
    expect(matchSourceCategories(['Dárky', 'Auto'], cats, display)).toEqual({
      Dárky: null,
      Auto: null,
    });
  });

  it('a full path disambiguates a repeated leaf', () => {
    expect(matchSourceCategories(['Jídlo:Dárky'], cats, display)).toEqual({
      'Jídlo:Dárky': 'u3',
    });
  });

  it('falls back to the leaf when the path names no parent it knows', () => {
    // Quicken writes Income:Salary; Moony's Salary has no parent, so the leaf decides.
    expect(matchSourceCategories(['Income:Kino', 'Auto:Dárky'], cats, display)).toEqual({
      'Income:Kino': 'u1',
      'Auto:Dárky': null,
    });
  });
});

describe('foldName', () => {
  it('ignores case, diacritics and runs of whitespace', () => {
    expect(foldName('  Žluťoučký   KŮŇ ')).toBe('zlutoucky kun');
  });
});

describe('sourceCategoryMapping', () => {
  it('sends the sources given a category, in the order of the panel', () => {
    expect(
      sourceCategoryMapping(['Food:Groceries', 'Auto', 'Kino'], {
        Kino: 'u1',
        Auto: null,
        'Food:Groceries': 'cat_groceries',
      })
    ).toEqual([
      { source: 'Food:Groceries', categoryId: 'cat_groceries' },
      { source: 'Kino', categoryId: 'u1' },
    ]);
  });

  it('leaves out sources of other statements and sources without a choice', () => {
    expect(sourceCategoryMapping(['Kino', 'Auto'], { Dárky: 'u2' })).toEqual([]);
  });
});
