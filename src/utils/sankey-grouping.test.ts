import { describe, it, expect } from 'vitest';
import {
  groupSmallFlows,
  nodeBox,
  showNodeLabel,
  MIN_NODE_HEIGHT,
  MIN_LABEL_HEIGHT,
} from './sankey-grouping';

const other = (count: number) => `Other (${count})`;

describe('groupSmallFlows', () => {
  it('merges flows under 2 % of the side total into one trailing "Other" node', () => {
    const flows = [
      { name: 'Salary', total: 900 },
      { name: 'Interest', total: 10 },
      { name: 'Rent', total: 80 },
      { name: 'Dividends', total: 5 },
    ];
    // total 995: Interest = 1.0 %, Dividends = 0.5 %
    expect(groupSmallFlows(flows, other)).toEqual([
      { name: 'Salary', total: 900 },
      { name: 'Rent', total: 80 },
      {
        name: 'Other (2)',
        total: 15,
        members: [
          { name: 'Interest', total: 10 },
          { name: 'Dividends', total: 5 },
        ],
      },
    ]);
  });

  it('keeps the order of the flows that stay', () => {
    const result = groupSmallFlows(
      [
        { name: 'B', total: 50 },
        { name: 'tiny1', total: 1 },
        { name: 'A', total: 48 },
        { name: 'tiny2', total: 1 },
      ],
      other
    );
    expect(result.map((f) => f.name)).toEqual(['B', 'A', 'Other (2)']);
  });

  it('lists the grouped flows largest first', () => {
    const result = groupSmallFlows(
      [
        { name: 'big', total: 1000 },
        { name: 'a', total: 5 },
        { name: 'b', total: 15 },
        { name: 'c', total: 10 },
      ],
      other
    );
    expect(result.at(-1)?.members?.map((m) => m.name)).toEqual(['b', 'c', 'a']);
  });

  it('keeps a flow of exactly 2 % (only strictly smaller flows are grouped)', () => {
    const flows = [
      { name: 'big', total: 98 },
      { name: 'edge', total: 2 },
    ];
    expect(groupSmallFlows(flows, other)).toEqual(flows);
  });

  it('does not wrap a single small flow in an "Other" node', () => {
    const flows = [
      { name: 'big', total: 995 },
      { name: 'tiny', total: 5 },
    ];
    expect(groupSmallFlows(flows, other)).toEqual(flows);
  });

  it('returns the flows unchanged when there is nothing to group', () => {
    const flows = [
      { name: 'a', total: 60 },
      { name: 'b', total: 40 },
    ];
    expect(groupSmallFlows(flows, other)).toEqual(flows);
    expect(groupSmallFlows([], other)).toEqual([]);
  });

  it('honours a custom threshold', () => {
    const result = groupSmallFlows(
      [
        { name: 'a', total: 80 },
        { name: 'b', total: 12 },
        { name: 'c', total: 8 },
      ],
      other,
      0.15
    );
    expect(result.map((f) => f.name)).toEqual(['a', 'Other (2)']);
    expect(result[1].total).toBe(20);
  });

  it('preserves the total value', () => {
    const flows = [
      { name: 'a', total: 700 },
      { name: 'b', total: 7 },
      { name: 'c', total: 3 },
      { name: 'd', total: 290 },
    ];
    const sum = (list: { total: number }[]) => list.reduce((s, f) => s + f.total, 0);
    expect(sum(groupSmallFlows(flows, other))).toBe(sum(flows));
  });
});

describe('nodeBox', () => {
  it('leaves a node that is tall enough alone', () => {
    expect(nodeBox(100, 40)).toEqual({ y: 100, height: 40 });
  });

  it('grows a thin node to the minimum height around its own centre', () => {
    const box = nodeBox(100, 1);
    expect(box.height).toBe(MIN_NODE_HEIGHT);
    expect(box.y + box.height / 2).toBeCloseTo(100.5);
  });
});

describe('showNodeLabel', () => {
  it('hides the label of a node thinner than the label height', () => {
    expect(showNodeLabel(MIN_LABEL_HEIGHT - 1)).toBe(false);
    expect(showNodeLabel(0)).toBe(false);
  });

  it('shows the label from the minimum height up', () => {
    expect(showNodeLabel(MIN_LABEL_HEIGHT)).toBe(true);
    expect(showNodeLabel(60)).toBe(true);
  });
});
