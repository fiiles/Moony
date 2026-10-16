import { describe, expect, it } from 'vitest';
import type { TFunction } from 'i18next';
import type { PerformanceSeries, StockTag } from '@shared/schema';
import { NO_SPLIT, type CompareGroup, type Part } from '@/utils/stock-performance';
import reportsEn from '@/i18n/locales/en/reports.json';
import {
  MAX_DRAWN,
  amountOf,
  buildRows,
  chartLines,
  figuresAt,
  fxEffect,
  groupName,
  partName,
  rankParts,
  rankedAt,
  sectorLabel,
  sumKnown,
  type BuildRowsInput,
  type RankedPart,
} from './performance-model';

function tag(id: string): StockTag {
  return { id, name: id.toUpperCase(), groupId: 'g1', createdAt: 0 } as unknown as StockTag;
}

function part(key: string, positionIds: string[], extra: Partial<Part> = {}): Part {
  return { key, positionIds, tag: null, sector: null, currency: null, isNone: false, ...extra };
}

describe('amountOf and sumKnown', () => {
  it('reads decimal text and rejects the rest', () => {
    expect(amountOf('1234.50')).toBe(1234.5);
    expect(amountOf('-0.5')).toBe(-0.5);
    expect(amountOf(null)).toBeNull();
    expect(amountOf(undefined)).toBeNull();
    expect(amountOf('abc')).toBeNull();
  });

  it('sums the known amounts and stays null when none is known', () => {
    expect(sumKnown([1, null, 2])).toBe(3);
    expect(sumKnown([null, null])).toBeNull();
    expect(sumKnown([])).toBeNull();
    expect(sumKnown([0])).toBe(0);
  });
});

describe('rankParts', () => {
  const values: Record<string, number | null> = {
    a: 10,
    b: 50,
    c: 30,
    d: 5,
    e: 70,
    f: null,
    n: 100,
  };
  const valueOf = (id: string) => values[id] ?? null;

  it('orders by value, largest first, and draws the first four', () => {
    const ranked = rankParts(
      [
        part('p1', ['a']),
        part('p2', ['b']),
        part('p3', ['c']),
        part('p4', ['d']),
        part('p5', ['e']),
      ],
      valueOf
    );
    expect(ranked.map((r) => r.part.key)).toEqual(['p5', 'p2', 'p3', 'p1', 'p4']);
    expect(ranked.map((r) => r.drawn)).toEqual([true, true, true, true, false]);
    expect(ranked.map((r) => r.color)).toEqual([
      'var(--s1)',
      'var(--s2)',
      'var(--s3)',
      'var(--s4)',
      null,
    ]);
    expect(MAX_DRAWN).toBe(4);
  });

  it('puts the part without a tag last and never draws it', () => {
    const ranked = rankParts(
      [part('none:g1', ['n'], { isNone: true }), part('p1', ['a']), part('p2', ['b'])],
      valueOf
    );
    expect(ranked.map((r) => r.part.key)).toEqual(['p2', 'p1', 'none:g1']);
    expect(ranked[2]).toMatchObject({ drawn: false, color: null, value: 100 });
  });

  it('sums the positions of a part and ranks an unknown value last among the regular ones', () => {
    const ranked = rankParts(
      [part('p1', ['f']), part('p2', ['a', 'b']), part('p3', ['d'])],
      valueOf
    );
    expect(ranked.map((r) => [r.part.key, r.value])).toEqual([
      ['p2', 60],
      ['p3', 5],
      ['p1', null],
    ]);
  });

  it('keeps the incoming order for equal values', () => {
    const ranked = rankParts([part('x', ['a']), part('y', ['a'])], valueOf);
    expect(ranked.map((r) => r.part.key)).toEqual(['x', 'y']);
  });
});

function series(
  key: string,
  twr: number[],
  contribution: number[] | null = null
): PerformanceSeries {
  return { key, twr, contribution, gain: null };
}

function ranked(p: Part, value: number | null, drawn: boolean, color: string | null): RankedPart {
  return { part: p, value, drawn, color };
}

function input(overrides: Partial<BuildRowsInput> = {}): BuildRowsInput {
  const growth = part('tag:growth', ['a', 'b'], { tag: tag('growth') });
  const value = part('tag:value', ['c'], { tag: tag('value') });
  const none = part('none:g1', ['d'], { isNone: true });
  return {
    series: [
      series('tag:growth', [0, 5, 10], [0, 3, 6]),
      series('tag:value', [0, -1, 2], [0, -0.5, 1]),
      series('none:g1', [0, 1, 1], [0, 0.2, 0.2]),
    ],
    portfolio: [0, 3, 8],
    ranked: [
      ranked(growth, 60, true, 'var(--s1)'),
      ranked(value, 30, true, 'var(--s2)'),
      ranked(none, 10, false, null),
    ],
    nameOf: (p) => (p.isNone ? 'No tag' : (p.tag?.name ?? p.key)),
    portfolioName: 'Whole portfolio',
    totalValue: 100,
    selectedValue: 100,
    filterActive: false,
    index: null,
    reportCurrency: 'CZK',
    ...overrides,
  };
}

describe('buildRows', () => {
  it('orders the parts, the portfolio, the index and the part without a tag', () => {
    const rows = buildRows(input({ index: { name: 'S&P 500', ticker: 'CSPX.L', twr: [0, 2, 4] } }));
    expect(rows.map((r) => r.id)).toEqual([
      'tag:growth',
      'tag:value',
      'portfolio',
      'index',
      'none:g1',
    ]);
    expect(rows.map((r) => r.kind)).toEqual(['part', 'part', 'portfolio', 'index', 'part']);
  });

  it('carries series, contribution, share and drawn state per part', () => {
    const [growth, , , none] = buildRows(input());
    expect(growth).toMatchObject({
      name: 'GROWTH',
      drawn: true,
      color: 'var(--s1)',
      quiet: false,
      twr: [0, 5, 10],
      contribution: [0, 3, 6],
      count: 2,
      value: 60,
      share: 0.6,
    });
    expect(growth.drawdown).toEqual([0, 0, 0]);
    expect(none).toMatchObject({
      name: 'No tag',
      isNone: true,
      drawn: false,
      quiet: true,
      share: 0.1,
    });
  });

  it('lists a part that is not drawn as quiet and without a colour', () => {
    const extra = part('tag:extra', ['e'], { tag: tag('extra') });
    const rows = buildRows(
      input({
        series: [...input().series, series('tag:extra', [0, 1, 2], [0, 0.1, 0.2])],
        ranked: [...input().ranked, ranked(extra, 5, false, null)],
      })
    );
    expect(rows.find((r) => r.id === 'tag:extra')).toMatchObject({
      drawn: false,
      color: null,
      quiet: true,
    });
  });

  it('skips a part the response has no series for', () => {
    const rows = buildRows(input({ series: [series('tag:growth', [0, 5, 10])] }));
    expect(rows.map((r) => r.id)).toEqual(['tag:growth', 'portfolio']);
  });

  it('draws the portfolio dashed unless "don\'t split" leaves it alone', () => {
    const mainOf = (overrides: Partial<BuildRowsInput>) =>
      buildRows(input(overrides)).find((r) => r.id === 'portfolio')?.main;
    expect(mainOf({})).toBe(false);
    expect(mainOf({ splitKind: 'tags' })).toBe(false);
    // a tag group with no part in the selection still says "dashed: the whole portfolio"
    expect(mainOf({ splitKind: 'tags', ranked: [], series: [] })).toBe(false);
    expect(mainOf({ splitKind: null, ranked: [], series: [] })).toBe(false);
    expect(mainOf({ splitKind: 'none', ranked: [], series: [] })).toBe(true);
  });

  it('keeps the portfolio the dashed reference next to the selection of "don\'t split"', () => {
    const selection = part('selection', ['a', 'b']);
    const rows = buildRows(
      input({
        splitKind: 'none',
        filterActive: true,
        series: [series('selection', [0, 4, 9], [0, 2, 5])],
        ranked: [ranked(selection, 60, true, 'var(--s1)')],
        selectedValue: 60,
        nameOf: (p) => (p.key === 'selection' ? 'Selection' : p.key),
      })
    );
    expect(rows.map((r) => r.id)).toEqual(['selection', 'portfolio']);
    expect(rows[0]).toMatchObject({
      kind: 'part',
      name: 'Selection',
      drawn: true,
      main: false,
      color: 'var(--s1)',
      quiet: false,
      tag: null,
      count: 2,
      share: 1,
      twr: [0, 4, 9],
      contribution: [0, 2, 5],
    });
    expect(rows[1].main).toBe(false);
  });

  it('shows the portfolio as 100 percent, or no share while a selection is active', () => {
    expect(buildRows(input()).find((r) => r.id === 'portfolio')).toMatchObject({
      share: 1,
      value: 100,
      contribution: null,
      color: 'var(--chart-line)',
    });
    expect(
      buildRows(input({ filterActive: true })).find((r) => r.id === 'portfolio')?.share
    ).toBeNull();
  });

  it('has no share without a selection value', () => {
    const rows = buildRows(input({ selectedValue: null }));
    expect(rows.every((r) => r.kind !== 'part' || r.share === null)).toBe(true);
  });

  it('attaches the native series of a part in another currency', () => {
    const usd = part('currency:USD', ['a'], { currency: 'USD' });
    const czk = part('currency:CZK', ['b'], { currency: 'CZK' });
    const rows = buildRows(
      input({
        series: [
          series('currency:USD', [0, 4, 8], [0, 2, 4]),
          series('currency:USD:native', [0, 6, 12], null),
          series('currency:CZK', [0, 1, 2], [0, 1, 2]),
        ],
        ranked: [ranked(usd, 70, true, 'var(--s1)'), ranked(czk, 30, true, 'var(--s2)')],
        nameOf: (p) => p.currency ?? p.key,
      })
    );
    expect(rows[0].native).toEqual([0, 6, 12]);
    expect(rows[1].native).toBeNull();
    expect(fxEffect(rows[0], 2)).toBe(-4);
    expect(fxEffect(rows[1], 2)).toBeNull();
  });

  it('computes drawdown from the running peak', () => {
    const rows = buildRows(input({ portfolio: [0, 10, 5, 12], series: [] }));
    const portfolio = rows.find((r) => r.id === 'portfolio');
    expect(portfolio?.drawdown[2]).toBeCloseTo((105 / 110 - 1) * 100, 6);
    expect(portfolio?.drawdown[3]).toBe(0);
  });
});

describe('chartLines', () => {
  it('draws the drawn parts, then the index, then the portfolio', () => {
    const rows = buildRows(
      input({ index: { name: 'MSCI World', ticker: 'IWDA.L', twr: [0, 1, 2] } })
    );
    const lines = chartLines(rows);
    expect(lines.map((l) => [l.id, l.variant])).toEqual([
      ['tag:growth', 'solid'],
      ['tag:value', 'solid'],
      ['index', 'index'],
      ['portfolio', 'reference'],
    ]);
    expect(lines[0]).toMatchObject({ name: 'GROWTH', color: 'var(--s1)', twr: [0, 5, 10] });
    expect(lines[3].color).toBe('var(--chart-line)');
  });

  it('has only the portfolio without parts', () => {
    const lines = chartLines(buildRows(input({ ranked: [], series: [] })));
    expect(lines.map((l) => l.id)).toEqual(['portfolio']);
  });

  it('draws the portfolio of "don\'t split" as the solid main line, the index dotted', () => {
    const none = { ranked: [], series: [], splitKind: 'none' as const };
    expect(chartLines(buildRows(input(none))).map((l) => [l.id, l.variant])).toEqual([
      ['portfolio', 'main'],
    ]);
    const withIndex = chartLines(
      buildRows(input({ ...none, index: { name: 'MSCI World', ticker: 'IWDA.L', twr: [0, 1, 2] } }))
    );
    expect(withIndex.map((l) => [l.id, l.variant])).toEqual([
      ['index', 'index'],
      ['portfolio', 'main'],
    ]);
    expect(withIndex[1].color).toBe('var(--chart-line)');
  });

  it('draws the selection of "don\'t split" solid in s1 against the dashed portfolio', () => {
    const selection = part('selection', ['a']);
    const lines = chartLines(
      buildRows(
        input({
          splitKind: 'none',
          filterActive: true,
          series: [series('selection', [0, 4, 9])],
          ranked: [ranked(selection, 10, true, 'var(--s1)')],
          nameOf: () => 'Selection',
        })
      )
    );
    expect(lines.map((l) => [l.id, l.variant, l.color])).toEqual([
      ['selection', 'solid', 'var(--s1)'],
      ['portfolio', 'reference', 'var(--chart-line)'],
    ]);
  });
});

describe('rankedAt', () => {
  it('orders the drawn parts by their value at a point', () => {
    const rows = buildRows(input());
    expect(rankedAt(rows, 2).map((r) => r.row.id)).toEqual(['tag:growth', 'tag:value']);
    expect(rankedAt(rows, 1).map((r) => r.twr)).toEqual([5, -1]);
    expect(rankedAt(rows, 9)).toEqual([]);
  });

  it('reads the selection of "don\'t split" as the one part, never the portfolio', () => {
    const rows = buildRows(
      input({
        splitKind: 'none',
        filterActive: true,
        series: [series('selection', [0, 4, 9])],
        ranked: [ranked(part('selection', ['a']), 10, true, 'var(--s1)')],
      })
    );
    expect(rankedAt(rows, 2).map((r) => [r.row.id, r.twr])).toEqual([['selection', 9]]);
    // no part (no filter): nothing to rank, the hero shows the drawdown instead
    expect(rankedAt(buildRows(input({ splitKind: 'none', ranked: [], series: [] })), 2)).toEqual(
      []
    );
  });
});

describe('figuresAt', () => {
  const performance = {
    portfolio: [0, 4, 10],
    mwr: [0, 5, 12],
    values: ['1000', '1200', '1500'],
    netFlows: ['0', '100', '200'],
  };

  it('reads the portfolio return, the money-weighted return and the change of value', () => {
    expect(figuresAt(performance, 2)).toEqual({
      twr: 10,
      mwr: 12,
      mwrGap: 2,
      change: 500,
      deposits: 200,
      market: 300,
    });
    expect(figuresAt(performance, 0)).toMatchObject({ twr: 0, change: 0, market: 0 });
  });

  it('has no change when a value is not convertible', () => {
    const f = figuresAt({ ...performance, values: [null, '1200', null] }, 2);
    expect(f).toMatchObject({ twr: 10, change: null, deposits: 200, market: null });
    expect(figuresAt({ ...performance, values: ['1000', null, '1500'] }, 1)?.change).toBeNull();
  });

  it('returns null past the last point', () => {
    expect(figuresAt(performance, 3)).toBeNull();
  });
});

describe('groupName and partName', () => {
  const tagGroup = {
    kind: 'tags',
    key: 'group:g1',
    group: { id: 'g1', name: 'Strategy', description: null, createdAt: 0 },
    tags: [],
  } as unknown as CompareGroup;
  const auto = (kind: 'none' | 'sector' | 'currency') =>
    ({ none: 'Nerozdělovat', sector: 'Sektor', currency: 'Měna' })[kind];
  const none = { tag: 'No tag', sector: 'No sector', selection: 'Selection' };

  it('names a tag group by itself and an automatic group by its label', () => {
    expect(groupName(tagGroup, auto)).toBe('Strategy');
    expect(groupName({ kind: 'sector', key: 'sector' }, auto)).toBe('Sektor');
    expect(groupName({ kind: 'currency', key: 'currency' }, auto)).toBe('Měna');
    expect(groupName(NO_SPLIT, auto)).toBe('Nerozdělovat');
  });

  it('names a part by its tag, sector or currency', () => {
    expect(partName(part('tag:a', [], { tag: tag('a') }), 'tags', none)).toBe('A');
    expect(partName(part('sector:Tech', [], { sector: 'Tech' }), 'sector', none)).toBe('Tech');
    expect(partName(part('currency:USD', [], { currency: 'USD' }), 'currency', none)).toBe('USD');
  });

  it('names the part without a tag or sector by the kind of group', () => {
    expect(partName(part('none:g', [], { isNone: true }), 'tags', none)).toBe('No tag');
    expect(partName(part('sector:', [], { isNone: true }), 'sector', none)).toBe('No sector');
    expect(partName(part('none:g', [], { isNone: true }), undefined, none)).toBe('No tag');
  });

  it('names the selection part of "don\'t split" whatever the kind of group', () => {
    const selection = part('selection', ['a', 'b']);
    expect(partName(selection, 'none', none)).toBe('Selection');
    expect(partName(selection, undefined, none)).toBe('Selection');
    // not through the sector translation
    expect(partName(selection, 'none', none, (name) => name.toUpperCase())).toBe('Selection');
  });

  it('shows a sector through the given translation, a tag or currency as it is', () => {
    const shout = (name: string) => name.toUpperCase();
    expect(partName(part('sector:Tech', [], { sector: 'Tech' }), 'sector', none, shout)).toBe(
      'TECH'
    );
    expect(partName(part('tag:a', [], { tag: tag('a') }), 'tags', none, shout)).toBe('A');
    expect(partName(part('currency:usd', [], { currency: 'usd' }), 'currency', none, shout)).toBe(
      'usd'
    );
    expect(partName(part('sector:', [], { isNone: true }), 'sector', none, shout)).toBe(
      'No sector'
    );
  });
});

describe('sectorLabel', () => {
  const dictionary: Record<string, string> = {
    'stocksAnalysis.sectors.technology': 'Technologie',
    'stocksAnalysis.sectors.financialServices': 'Finanční služby',
    'stocksAnalysis.sectors.healthcare': 'Zdravotnictví',
    'stocksAnalysis.sectors.consumerCyclical': 'Cyklická spotřeba',
    'stocksAnalysis.sectors.consumerDefensive': 'Základní spotřeba',
    'stocksAnalysis.sectors.energy': 'Energetika',
    'stocksAnalysis.sectors.industrials': 'Průmysl',
    'stocksAnalysis.sectors.basicMaterials': 'Základní suroviny',
    'stocksAnalysis.sectors.realEstate': 'Nemovitosti',
    'stocksAnalysis.sectors.utilities': 'Veřejné služby',
    'stocksAnalysis.sectors.communicationServices': 'Komunikační služby',
  };
  const t = ((key: string) => dictionary[key] ?? `?${key}`) as unknown as TFunction;

  it('translates each of the eleven Yahoo sectors', () => {
    expect(sectorLabel('Technology', t)).toBe('Technologie');
    expect(sectorLabel('Financial Services', t)).toBe('Finanční služby');
    expect(sectorLabel('Healthcare', t)).toBe('Zdravotnictví');
    expect(sectorLabel('Consumer Cyclical', t)).toBe('Cyklická spotřeba');
    expect(sectorLabel('Consumer Defensive', t)).toBe('Základní spotřeba');
    expect(sectorLabel('Energy', t)).toBe('Energetika');
    expect(sectorLabel('Industrials', t)).toBe('Průmysl');
    expect(sectorLabel('Basic Materials', t)).toBe('Základní suroviny');
    expect(sectorLabel('Real Estate', t)).toBe('Nemovitosti');
    expect(sectorLabel('Utilities', t)).toBe('Veřejné služby');
    expect(sectorLabel('Communication Services', t)).toBe('Komunikační služby');
  });

  it('matches ignoring case and spaces', () => {
    expect(sectorLabel('technology', t)).toBe('Technologie');
    expect(sectorLabel('FINANCIAL SERVICES', t)).toBe('Finanční služby');
    expect(sectorLabel('  real   estate ', t)).toBe('Nemovitosti');
    expect(sectorLabel('BasicMaterials', t)).toBe('Základní suroviny');
  });

  it('shows an unknown name as it came', () => {
    expect(sectorLabel('Crypto Mining', t)).toBe('Crypto Mining');
    expect(sectorLabel('', t)).toBe('');
  });

  it('maps every sector to a key the English locale carries', () => {
    const sectors: Record<string, string> = reportsEn.stocksAnalysis.sectors;
    const lookup = ((key: string) =>
      sectors[key.replace('stocksAnalysis.sectors.', '')] ?? `?${key}`) as unknown as TFunction;
    const yahoo = [
      'Technology',
      'Financial Services',
      'Healthcare',
      'Consumer Cyclical',
      'Consumer Defensive',
      'Energy',
      'Industrials',
      'Basic Materials',
      'Real Estate',
      'Utilities',
      'Communication Services',
    ];
    for (const name of yahoo) {
      // The English copy is the Yahoo name; a key the file lacks would read "?stocksAnalysis…"
      expect(sectorLabel(name, lookup).toLowerCase()).toBe(name.toLowerCase());
    }
    expect(Object.keys(sectors)).toHaveLength(yahoo.length);
  });
});
