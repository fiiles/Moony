import { describe, expect, it } from 'vitest';
import type { RemovalPlan, RemovalPlanDevice } from '@shared/schema';
import { pruneAlsoRemove, toggleId } from '@/utils/sync-removal';
import { applyDefaultOnce } from './use-default-also-remove';

const dev = (id: string): RemovalPlanDevice => ({
  id,
  name: id,
  platform: 'macos',
  joinedAt: 1,
  thisDevice: false,
  addedAt: 1,
  addedBy: 'Mac',
});

const plan = (over: Partial<RemovalPlan> = {}): RemovalPlan => ({
  removing: [dev('b')],
  staying: [dev('a')],
  droppingOut: [],
  addedByRemoving: [dev('d'), dev('e')],
  tickedByDefault: ['d', 'e'],
  replacesKit: 'no',
  kitNotUpdated: false,
  consent: 'abc',
  ...over,
});

/** A dialog's `alsoRemove` state and the ref the hook keeps, driven by the plans that arrive. */
function dialog() {
  const applied = { current: false };
  let alsoRemove: string[] = [];
  return {
    arrive: (next: RemovalPlan | undefined) =>
      applyDefaultOnce(applied, next, (ids) => (alsoRemove = ids)),
    tick: (id: string) => (alsoRemove = toggleId(alsoRemove, id)),
    ticks: () => alsoRemove,
  };
}

describe('applyDefaultOnce', () => {
  it('waits for the first plan, then ticks the recent devices', () => {
    const view = dialog();
    view.arrive(undefined);
    expect(view.ticks()).toEqual([]);
    view.arrive(plan());
    expect(view.ticks()).toEqual(['d', 'e']);
  });

  it('keeps a device the person unticked unticked when the plan reloads', () => {
    const view = dialog();
    view.arrive(plan());
    view.tick('d');
    expect(view.ticks()).toEqual(['e']);
    view.arrive(plan({ consent: 'def' }));
    view.arrive(plan({ consent: 'ghi' }));
    expect(view.ticks()).toEqual(['e']);
  });

  it('later plans only prune the ticks', () => {
    const view = dialog();
    view.arrive(plan());
    const later = plan({ addedByRemoving: [dev('e')] });
    view.arrive(later);
    expect(view.ticks()).toEqual(['d', 'e']);
    expect(pruneAlsoRemove(view.ticks(), later)).toEqual(['e']);
  });

  it('applies nothing when no device is recent, and not again when one turns up later', () => {
    const view = dialog();
    view.arrive(plan({ tickedByDefault: [] }));
    expect(view.ticks()).toEqual([]);
    view.arrive(plan());
    expect(view.ticks()).toEqual([]);
  });
});
