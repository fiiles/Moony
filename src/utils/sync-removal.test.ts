import { describe, expect, it } from 'vitest';
import type { RecoveryKit, RemovalPlan, RemovalPlanDevice, SyncStatus } from '@shared/schema';
import {
  canKeepDropping,
  dropReasonKey,
  kitLineKey,
  outcomeView,
  planMakesKit,
  pruneAlsoRemove,
  removalAvailable,
  removalRequest,
  removalTitleName,
  removalTitleNames,
  toggleId,
} from './sync-removal';

const dev = (id: string, name = id): RemovalPlanDevice => ({
  id,
  name,
  platform: 'macos',
  joinedAt: 1,
  thisDevice: false,
});

function plan(over: Partial<RemovalPlan> = {}): RemovalPlan {
  return {
    removing: [dev('b', 'Phone')],
    staying: [dev('a', 'Mac')],
    droppingOut: [],
    addedByRemoving: [dev('d'), dev('e')],
    replacesKit: 'no',
    kitNotUpdated: false,
    consent: 'abc',
    ...over,
  };
}

const kit: RecoveryKit = {
  recoveryCode: 'AAAAA',
  qrSvg: '<svg/>',
  relayUrl: null,
  createdAt: 1,
  createdBy: 'Mac',
};

describe('kitLineKey', () => {
  it('maps every replacesKit value to its line', () => {
    expect(kitLineKey(plan({ replacesKit: 'creatorRemoved' }))).toBe('remove.kit.creatorRemoved');
    expect(kitLineKey(plan({ replacesKit: 'cloud' }))).toBe('remove.kit.cloud');
    expect(kitLineKey(plan({ replacesKit: 'missing' }))).toBe('remove.kit.missing');
    expect(kitLineKey(plan({ replacesKit: 'asked' }))).toBeNull();
    expect(kitLineKey(plan({ replacesKit: 'no' }))).toBeNull();
  });

  it('says the kit is not updated only when no kit is involved', () => {
    expect(kitLineKey(plan({ replacesKit: 'no', kitNotUpdated: true }))).toBe(
      'remove.kit.notUpdated'
    );
  });

  it('makes a new kit unless the kit stays', () => {
    expect(planMakesKit(plan())).toBe(false);
    for (const replacesKit of ['creatorRemoved', 'cloud', 'asked', 'missing'] as const) {
      expect(planMakesKit(plan({ replacesKit }))).toBe(true);
    }
  });
});

describe('dropping out', () => {
  it('keys the reasons and offers "Je moje" for a late vouch only', () => {
    expect(dropReasonKey('issuerRemoved')).toBe('remove.dropReason.issuerRemoved');
    expect(dropReasonKey('copy')).toBe('remove.dropReason.unverified');
    expect(canKeepDropping('lateVouch')).toBe(true);
    expect(canKeepDropping('unverified')).toBe(false);
    expect(canKeepDropping('issuerRemoved')).toBe(false);
    expect(canKeepDropping('copy')).toBe(false);
  });
});

describe('ticks', () => {
  it('toggles an id', () => {
    expect(toggleId([], 'd')).toEqual(['d']);
    expect(toggleId(['d', 'e'], 'd')).toEqual(['e']);
  });

  it('drops a tick the reloaded plan no longer lists', () => {
    expect(pruneAlsoRemove(['d', 'x'], plan())).toEqual(['d']);
    expect(pruneAlsoRemove(['d', 'x'], undefined)).toEqual(['d', 'x']);
  });

  it('builds the request from the plan: consent, pruned ticks, no new kit', () => {
    expect(removalRequest(plan(), ['b'], ['e', 'gone'])).toEqual({
      deviceIds: ['b'],
      alsoRemove: ['e'],
      newKit: false,
      consent: 'abc',
    });
  });

  it('asks for a new kit with nobody named when the new-kit dialog says so', () => {
    expect(removalRequest(plan({ replacesKit: 'asked' }), [], ['d'], true)).toEqual({
      deviceIds: [],
      alsoRemove: ['d'],
      newKit: true,
      consent: 'abc',
    });
  });
});

describe('removalTitleName', () => {
  it('names the first device asked for', () => {
    expect(
      removalTitleName(plan({ removing: [dev('d', 'Tablet'), dev('b', 'Phone')] }), ['b'])
    ).toBe('Phone');
    expect(removalTitleName(undefined, ['b'])).toBeNull();
  });
});

describe('removalTitleNames', () => {
  it('joins the names of every device asked for', () => {
    const p = plan({ removing: [dev('d', 'Tablet'), dev('b', 'Phone'), dev('c', 'Mac')] });
    expect(removalTitleNames(p, ['b', 'c'])).toBe('Phone, Mac');
    expect(removalTitleNames(p, ['x'])).toBeNull();
    expect(removalTitleNames(undefined, ['b'])).toBeNull();
  });
});

describe('outcomeView', () => {
  it('shows the new kit of a done or pending removal, and never of the others', () => {
    expect(outcomeView({ kind: 'done', by: null, kit })).toEqual({
      toast: 'removed',
      by: null,
      kit,
      kitPending: false,
    });
    expect(outcomeView({ kind: 'pending', by: null, kit })).toMatchObject({
      toast: 'pending',
      kit,
      kitPending: true,
    });
    expect(outcomeView({ kind: 'done', by: null, kit: null }).kit).toBeNull();
    expect(outcomeView({ kind: 'alreadyRemoved', by: 'Mac', kit: null })).toMatchObject({
      toast: 'alreadyRemoved',
      by: 'Mac',
      kit: null,
    });
    expect(outcomeView({ kind: 'removedHere', by: 'Mac', kit: null })).toMatchObject({
      toast: 'removedHere',
      kit: null,
    });
  });
});

describe('removalAvailable', () => {
  const status = (over: Partial<SyncStatus>) => over as SyncStatus;
  it('needs a supported vault and no key change of ours waiting', () => {
    expect(removalAvailable(undefined)).toBe(false);
    expect(removalAvailable(status({ removalSupported: false, removalPending: false }))).toBe(
      false
    );
    expect(removalAvailable(status({ removalSupported: true, removalPending: true }))).toBe(false);
    expect(removalAvailable(status({ removalSupported: true, removalPending: false }))).toBe(true);
  });
});
