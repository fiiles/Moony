import { describe, expect, it } from 'vitest';
import type { Part } from '@/utils/stock-performance';
import {
  barLayout,
  contributionRows,
  partItems,
  positionItems,
  type ContributionItem,
} from './contribution-model';
import type { RankedPart } from './performance-model';

function item(id: string, points: number, extra: Partial<ContributionItem> = {}): ContributionItem {
  return {
    id,
    name: id,
    ticker: null,
    positions: null,
    points,
    periodReturn: null,
    muted: false,
    folded: null,
    ...extra,
  };
}

describe('contributionRows', () => {
  it('orders positions by the size of their effect, whatever the sign', () => {
    const rows = contributionRows(
      [item('a', 1.5), item('b', -4), item('c', 2.5)],
      'magnitude',
      'Other'
    );
    expect(rows.map((r) => r.id)).toEqual(['b', 'c', 'a']);
  });

  it('orders parts best first', () => {
    const rows = contributionRows(
      [item('a', 1.5), item('b', -4), item('c', 2.5)],
      'value',
      'Other'
    );
    expect(rows.map((r) => r.id)).toEqual(['c', 'a', 'b']);
  });

  it('keeps nine rows as they are', () => {
    const items = Array.from({ length: 9 }, (_, i) => item(`p${i}`, i + 1));
    const rows = contributionRows(items, 'magnitude', 'Other');
    expect(rows).toHaveLength(9);
    expect(rows.every((r) => r.id !== 'other')).toBe(true);
  });

  it('folds the smallest effects of a longer list into one muted row at the end', () => {
    const items = [
      item('a', 5),
      item('b', -4),
      item('c', 3),
      item('d', 2),
      item('e', 1.5),
      item('f', 1),
      item('g', 0.8),
      item('h', 0.6),
      item('i', 0.3),
      item('j', -0.2),
    ];
    const rows = contributionRows(items, 'magnitude', 'Other');
    expect(rows).toHaveLength(9);
    expect(rows.slice(0, 8).map((r) => r.id)).toEqual(['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h']);
    const other = rows[8];
    expect(other).toMatchObject({ id: 'other', name: 'Other', muted: true, periodReturn: null });
    expect(other.points).toBeCloseTo(0.1);
    // Two positions stand behind it
    expect(other.positions).toBe(2);
  });

  it('counts the positions of folded parts', () => {
    const parts = Array.from({ length: 10 }, (_, i) =>
      item(`t${i}`, 10 - i, { positions: i < 8 ? 1 : 3 })
    );
    const other = contributionRows(parts, 'value', 'Other').at(-1);
    expect(other?.positions).toBe(6);
  });

  describe('the folded items', () => {
    const long = [
      item('a', 5),
      item('b', -4),
      item('c', 3),
      item('d', 2),
      item('e', 1.5),
      item('f', 1),
      item('g', 0.8),
      item('h', 0.6),
      item('i', -0.3),
      item('j', 0.2),
      item('k', -0.1),
    ];

    it('are kept on the rest row, best contribution first and losses last', () => {
      const other = contributionRows(long, 'magnitude', 'Other').at(-1);
      expect(other?.folded?.map((r) => r.id)).toEqual(['j', 'k', 'i']);
    });

    it('add up to the rest row and stand for as many positions as it counts', () => {
      const other = contributionRows(long, 'magnitude', 'Other').at(-1);
      const folded = other?.folded ?? [];
      expect(folded.reduce((sum, r) => sum + r.points, 0)).toBeCloseTo(other?.points ?? NaN);
      expect(folded.reduce((sum, r) => sum + (r.positions ?? 1), 0)).toBe(other?.positions);
    });

    it('keep their own figures, so the list can show them like the card does', () => {
      const parts = Array.from({ length: 10 }, (_, i) =>
        item(`t${i}`, 10 - i, { positions: 2, periodReturn: i, muted: i === 9, ticker: null })
      );
      const other = contributionRows(parts, 'value', 'Other').at(-1);
      expect(other?.folded?.map((r) => r.id)).toEqual(['t8', 't9']);
      expect(other?.folded?.[1]).toMatchObject({ positions: 2, periodReturn: 9, muted: true });
    });

    it('stay sorted when the rows themselves are ordered by size', () => {
      // The card orders positions by magnitude: the rest holds small gains and small losses mixed
      const other = contributionRows(long, 'magnitude', 'Other').at(-1);
      const points = other?.folded?.map((r) => r.points) ?? [];
      expect(points).toEqual([...points].sort((x, y) => y - x));
    });

    it('exist only on the rest row', () => {
      const rows = contributionRows(long, 'magnitude', 'Other');
      expect(rows.slice(0, -1).every((r) => r.folded === null)).toBe(true);
      expect(rows.at(-1)?.folded).toHaveLength(3);
    });

    it('are absent when nothing is folded', () => {
      const rows = contributionRows(long.slice(0, 9), 'magnitude', 'Other');
      expect(rows.every((r) => r.folded === null)).toBe(true);
    });
  });
});

describe('barLayout', () => {
  it('puts the zero line at the left edge when everything is positive', () => {
    const { zero, bars } = barLayout([2, 4]);
    expect(zero).toBe(0);
    expect(bars[0]).toEqual({ left: 0, width: 50 });
    expect(bars[1]).toEqual({ left: 0, width: 100 });
  });

  it('shares one scale between both signs', () => {
    const { zero, bars } = barLayout([3, -1]);
    expect(zero).toBeCloseTo(25);
    expect(bars[0].left).toBeCloseTo(25);
    expect(bars[0].width).toBeCloseTo(75);
    // The negative bar ends at the zero line and grows to its left
    expect(bars[1].left).toBeCloseTo(0);
    expect(bars[1].width).toBeCloseTo(25);
  });

  it('leaves a mark for a value that rounds to nothing and survives all zeros', () => {
    expect(barLayout([0, 0]).bars.every((b) => b.width === 0.6)).toBe(true);
    expect(barLayout([]).bars).toEqual([]);
    expect(barLayout([-2]).zero).toBe(100);
  });
});

describe('positionItems', () => {
  it('lists the selected positions the response knows', () => {
    const figures = new Map([
      ['a', { contribution: 2.5, twr: 12 }],
      ['b', { contribution: -0.5, twr: -8 }],
    ]);
    const items = positionItems(
      [
        { id: 'b', companyName: 'Beta', ticker: 'BET' },
        { id: 'x', companyName: 'Unknown', ticker: 'UNK' },
        { id: 'a', companyName: 'Alfa', ticker: 'ALF' },
      ],
      figures
    );
    expect(items.map((i) => i.id)).toEqual(['b', 'a']);
    expect(items[1]).toMatchObject({
      name: 'Alfa',
      ticker: 'ALF',
      points: 2.5,
      periodReturn: 12,
      muted: false,
      positions: null,
      folded: null,
    });
  });
});

describe('partItems', () => {
  function part(key: string, ids: string[], extra: Partial<Part> = {}): Part {
    return {
      key,
      positionIds: ids,
      tag: null,
      sector: null,
      currency: null,
      isNone: false,
      ...extra,
    };
  }
  const ranked = (p: Part, drawn: boolean): RankedPart => ({
    part: p,
    value: 1,
    drawn,
    color: drawn ? 'var(--s1)' : null,
  });
  const series = new Map([
    ['tag:a', { twr: [0, 5, 9], contribution: [0, 2, 4] }],
    ['tag:b', { twr: [0, 1, -3], contribution: [0, 0.5, -1] }],
    ['none:g', { twr: [0, 0, 1], contribution: [0, 0, 0.2] }],
    ['sector:Tech:native', { twr: [0, 1, 2], contribution: null }],
  ]);

  it('reads the last point of each part and mutes the quiet ones', () => {
    const items = partItems(
      [
        ranked(part('tag:a', ['p1', 'p2']), true),
        ranked(part('tag:b', ['p3']), false),
        ranked(part('none:g', ['p4'], { isNone: true }), false),
      ],
      series,
      (p) => p.key
    );
    expect(items).toEqual([
      {
        id: 'tag:a',
        name: 'tag:a',
        ticker: null,
        positions: 2,
        points: 4,
        periodReturn: 9,
        muted: false,
        folded: null,
      },
      {
        id: 'tag:b',
        name: 'tag:b',
        ticker: null,
        positions: 1,
        points: -1,
        periodReturn: -3,
        muted: true,
        folded: null,
      },
      {
        id: 'none:g',
        name: 'none:g',
        ticker: null,
        positions: 1,
        points: 0.2,
        periodReturn: 1,
        muted: true,
        folded: null,
      },
    ]);
  });

  it('leaves out a part without a series or without a contribution', () => {
    const items = partItems(
      [
        ranked(part('tag:missing', ['p1']), true),
        ranked(part('sector:Tech:native', ['p2']), true),
        ranked(part('tag:a', ['p3']), true),
      ],
      series,
      (p) => p.key
    );
    expect(items.map((i) => i.id)).toEqual(['tag:a']);
  });
});
