import { describe, expect, it } from 'vitest';
import type { TagMetrics } from '@shared/schema';
import { buildTagShareRows } from './tag-share';

function metric(
  id: string,
  name: string,
  totalValue: number,
  portfolioPercent: number
): TagMetrics {
  return {
    tag: { id, name, color: id === 'a' ? '#123456' : null, groupId: null, createdAt: 0 },
    totalValue,
    totalCost: 0,
    gainLoss: 0,
    gainLossPercent: 0,
    estimatedYearlyDividend: 0,
    portfolioPercent,
    holdingsCount: 1,
  };
}

describe('buildTagShareRows', () => {
  it('sorts by value, largest first', () => {
    const rows = buildTagShareRows([
      metric('a', 'Growth', 100, 10),
      metric('b', 'Value', 600, 60),
      metric('c', 'Dividend', 300, 30),
    ]);
    expect(rows.map((r) => r.name)).toEqual(['Value', 'Dividend', 'Growth']);
  });

  it('keeps value, percent and colour of each tag', () => {
    const [row] = buildTagShareRows([metric('a', 'Growth', 250, 25)]);
    expect(row).toMatchObject({
      id: 'a',
      name: 'Growth',
      color: '#123456',
      value: 250,
      percent: 25,
    });
  });

  it('scales the bars to the largest tag', () => {
    const rows = buildTagShareRows([metric('a', 'Small', 50, 5), metric('b', 'Big', 200, 20)]);
    expect(rows.find((r) => r.id === 'b')?.barPercent).toBe(100);
    expect(rows.find((r) => r.id === 'a')?.barPercent).toBe(25);
  });

  it('breaks ties by name', () => {
    const rows = buildTagShareRows([metric('b', 'Beta', 10, 1), metric('a', 'Alpha', 10, 1)]);
    expect(rows.map((r) => r.name)).toEqual(['Alpha', 'Beta']);
  });

  it('gives empty bars when no tag holds any value', () => {
    const rows = buildTagShareRows([metric('a', 'Growth', 0, 0), metric('b', 'Value', 0, 0)]);
    expect(rows.every((r) => r.barPercent === 0)).toBe(true);
  });

  it('does not mutate the input', () => {
    const input = [metric('a', 'A', 1, 1), metric('b', 'B', 2, 2)];
    buildTagShareRows(input);
    expect(input.map((m) => m.tag.id)).toEqual(['a', 'b']);
  });

  it('returns nothing for no tags', () => {
    expect(buildTagShareRows([])).toEqual([]);
  });
});
