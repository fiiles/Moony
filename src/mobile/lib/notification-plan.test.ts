import { describe, expect, it } from 'vitest';
import type { Milestone, MilestoneKind } from '@shared/schema';
import {
  MAX_PENDING,
  NOTIFY_HOUR,
  buildNotificationPlan,
  isAfter,
  localTimeOf,
  nextUpkeepDate,
  notificationId,
  planSignature,
  scheduleDateString,
  utcDayToLocalTime,
  type LocalTime,
  type PlannedNotification,
} from './notification-plan';
import { DEFAULT_NOTIFICATION_PREFS, type NotificationPrefs } from './notification-prefs';

const NOW: LocalTime = { year: 2026, month: 10, day: 17, hour: 10, minute: 0 };

/** The UTC day (unix seconds, midnight) of a calendar date; `month` is 1–12. */
const utcDay = (year: number, month: number, day: number) => Date.UTC(year, month - 1, day) / 1000;
const at = (year: number, month: number, day: number, hour = 9, minute = 0): LocalTime => ({
  year,
  month,
  day,
  hour,
  minute,
});

function milestone(kind: MilestoneKind, patch: Partial<Milestone> = {}): Milestone {
  return {
    key: `${kind}:test`,
    kind,
    stage: 'now',
    tone: 'action',
    sourceId: null,
    title: '',
    dueDay: null,
    actionDay: null,
    sinceDay: null,
    amount: null,
    currency: null,
    referenceAmount: null,
    direction: null,
    count: null,
    ...patch,
  };
}

/** Only the deadline reminders: the monthly upkeep and the backup reminder are off. */
const DEADLINES_ONLY: NotificationPrefs = {
  ...DEFAULT_NOTIFICATION_PREFS,
  upkeep: false,
  backup: false,
};
const NOTHING: NotificationPrefs = { ...DEADLINES_ONLY, deadlines: false };

function plan(patch: {
  now?: LocalTime;
  milestones?: Milestone[];
  mutedKinds?: MilestoneKind[];
  prefs?: NotificationPrefs;
}): PlannedNotification[] {
  return buildNotificationPlan({
    now: patch.now ?? NOW,
    milestones: patch.milestones ?? [],
    mutedKinds: patch.mutedKinds ?? [],
    prefs: patch.prefs ?? DEADLINES_ONLY,
  });
}

describe('local time helpers', () => {
  it('reads the local components of a date', () => {
    // Built from local components, so the expectation holds in any time zone
    expect(localTimeOf(new Date(2026, 9, 17, 10, 30, 45))).toEqual(at(2026, 10, 17, 10, 30));
  });

  it('orders local times by date, then hour, then minute', () => {
    expect(isAfter(at(2026, 10, 18, 9), at(2026, 10, 17, 23, 59))).toBe(true);
    expect(isAfter(at(2026, 10, 17, 10, 1), at(2026, 10, 17, 10, 0))).toBe(true);
    expect(isAfter(at(2027, 1, 1, 0), at(2026, 12, 31, 23, 59))).toBe(true);
    expect(isAfter(at(2026, 10, 17, 10, 0), at(2026, 10, 17, 10, 0))).toBe(false);
    expect(isAfter(at(2026, 10, 17, 9, 59), at(2026, 10, 17, 10, 0))).toBe(false);
  });

  it('turns a UTC day into its calendar date at the given local hour', () => {
    expect(utcDayToLocalTime(utcDay(2026, 11, 15), 9)).toEqual(at(2026, 11, 15, 9, 0));
    expect(utcDayToLocalTime(utcDay(2026, 12, 31), NOTIFY_HOUR)).toEqual(at(2026, 12, 31, 9, 0));
    expect(utcDayToLocalTime(utcDay(2027, 3, 1), 14)).toEqual(at(2027, 3, 1, 14, 0));
  });

  it('emits local digits with a literal Z', () => {
    expect(scheduleDateString({ year: 2026, month: 11, day: 2, hour: 9, minute: 0 })).toBe(
      '2026-11-02T09:00:00.000Z'
    );
    expect(scheduleDateString({ year: 2027, month: 1, day: 5, hour: 14, minute: 7 })).toBe(
      '2027-01-05T14:07:00.000Z'
    );
  });

  it('adds the seconds of a time that has them', () => {
    expect(
      scheduleDateString({ year: 2026, month: 11, day: 2, hour: 9, minute: 5, second: 42 })
    ).toBe('2026-11-02T09:05:42.000Z');
  });
});

describe('buildNotificationPlan: deadlines', () => {
  it('schedules T-14 and T-1 at 09:00 local on the action day of an unmuted deadline', () => {
    const m = milestone('insurance_anniversary', {
      key: 'insurance_anniversary:policy-1:2027',
      title: 'Allianz Auto',
      actionDay: utcDay(2026, 11, 15),
      // The anniversary itself is later: the notice deadline is what counts
      dueDay: utcDay(2026, 12, 1),
    });
    expect(plan({ milestones: [m] })).toEqual([
      {
        id: notificationId('deadline14', m.key),
        kind: 'deadline14',
        at: at(2026, 11, 1, 9, 0),
        title: { key: 'notifications.deadline.title', params: { title: 'Allianz Auto' } },
        body: {
          key: 'notifications.deadline.in14',
          params: { whatKey: 'notifications.deadline.kind.insurance_anniversary' },
        },
      },
      {
        id: notificationId('deadline1', m.key),
        kind: 'deadline1',
        at: at(2026, 11, 14, 9, 0),
        title: { key: 'notifications.deadline.title', params: { title: 'Allianz Auto' } },
        body: {
          key: 'notifications.deadline.tomorrow',
          params: { whatKey: 'notifications.deadline.kind.insurance_anniversary' },
        },
      },
    ]);
  });

  it('uses dueDay when actionDay is null and skips muted kinds and info-tone items', () => {
    const fixation = milestone('loan_fixation_end', {
      key: 'loan_fixation_end:loan-1',
      title: 'Hypotéka',
      dueDay: utcDay(2026, 12, 20),
    });
    const muted = milestone('bond_maturity', {
      key: 'bond_maturity:bond-1',
      dueDay: utcDay(2026, 12, 20),
    });
    const passedNotice = milestone('insurance_anniversary', {
      key: 'insurance_anniversary:policy-2',
      tone: 'info',
      dueDay: utcDay(2026, 12, 20),
    });
    const notADeadline = milestone('insurance_payment', {
      key: 'insurance_payment:policy-3',
      dueDay: utcDay(2026, 12, 20),
    });
    const undated = milestone('account_termination', { key: 'account_termination:acc-1' });

    const result = plan({
      milestones: [fixation, muted, passedNotice, notADeadline, undated],
      mutedKinds: ['bond_maturity'],
    });
    expect(result.map((p) => [p.kind, p.at])).toEqual([
      ['deadline14', at(2026, 12, 6)],
      ['deadline1', at(2026, 12, 19)],
    ]);
    expect(result[0].body.params).toEqual({
      whatKey: 'notifications.deadline.kind.loan_fixation_end',
    });
  });

  it('plans no deadline reminder while the preference is off', () => {
    const m = milestone('insurance_end', { dueDay: utcDay(2026, 12, 20) });
    expect(plan({ milestones: [m], prefs: NOTHING })).toEqual([]);
  });

  it('names the kind through a key for every deadline kind', () => {
    const kinds: MilestoneKind[] = [
      'insurance_anniversary',
      'insurance_end',
      'loan_fixation_end',
      'bond_maturity',
      'account_termination',
      'savings_rate_end',
    ];
    const result = plan({
      milestones: kinds.map((kind) => milestone(kind, { dueDay: utcDay(2027, 3, 20) })),
    });
    const names = new Set(result.map((p) => p.body.params?.whatKey));
    expect(names).toEqual(new Set(kinds.map((kind) => `notifications.deadline.kind.${kind}`)));
  });

  it('drops items that are not after now and sorts the rest', () => {
    const late = milestone('bond_maturity', {
      key: 'bond_maturity:late',
      dueDay: utcDay(2026, 11, 30),
    });
    // T-14 is 11 October (past), T-1 is 24 October (ahead)
    const soon = milestone('insurance_end', {
      key: 'insurance_end:soon',
      dueDay: utcDay(2026, 10, 25),
    });
    expect(plan({ milestones: [late, soon] }).map((p) => [p.kind, p.at])).toEqual([
      ['deadline1', at(2026, 10, 24)],
      ['deadline14', at(2026, 11, 16)],
      ['deadline1', at(2026, 11, 29)],
    ]);
  });

  it('drops an item at exactly now', () => {
    // T-1 of 18 October is 17 October 09:00
    const m = milestone('insurance_end', { dueDay: utcDay(2026, 10, 18) });
    const result = (now: LocalTime) => plan({ now, milestones: [m] });
    expect(result(at(2026, 10, 17, 9, 0))).toEqual([]);
    expect(result(at(2026, 10, 17, 8, 59)).map((p) => p.kind)).toEqual(['deadline1']);
  });

  it('breaks ties between items at the same time by id, whatever the milestone order', () => {
    const a = milestone('insurance_end', { key: 'insurance_end:a', dueDay: utcDay(2027, 3, 20) });
    const b = milestone('bond_maturity', { key: 'bond_maturity:b', dueDay: utcDay(2027, 3, 20) });
    expect(plan({ milestones: [a, b] })).toEqual(plan({ milestones: [b, a] }));
  });
});

describe('nextUpkeepDate', () => {
  it('plans this month when the day is still ahead, next month otherwise', () => {
    expect(nextUpkeepDate(at(2026, 2, 28, 10, 0), 28)).toEqual(at(2026, 3, 28, 9, 0));
    expect(nextUpkeepDate(at(2026, 10, 17, 10, 0), 20)).toEqual(at(2026, 10, 20, 9, 0));
  });

  it('is next month when today is the day and 09:00 has passed or is now', () => {
    expect(nextUpkeepDate(at(2026, 10, 17, 9, 0), 17)).toEqual(at(2026, 11, 17, 9, 0));
    expect(nextUpkeepDate(at(2026, 10, 17, 8, 59), 17)).toEqual(at(2026, 10, 17, 9, 0));
  });

  it('rolls December over into January', () => {
    expect(nextUpkeepDate(at(2026, 12, 28, 10, 0), 15)).toEqual(at(2027, 1, 15, 9, 0));
  });
});

describe('buildNotificationPlan: monthly upkeep', () => {
  it('plans one upkeep reminder at the next upkeep day while it is on', () => {
    const prefs: NotificationPrefs = { ...NOTHING, upkeep: true, upkeepDay: 20 };
    expect(plan({ prefs })).toEqual([
      {
        id: notificationId('upkeep', 'upkeep'),
        kind: 'upkeep',
        at: at(2026, 10, 20, 9, 0),
        title: { key: 'notifications.upkeep.title' },
        body: { key: 'notifications.upkeep.body' },
      },
    ]);
    expect(plan({ prefs: { ...prefs, upkeep: false } })).toEqual([]);
  });
});

describe('buildNotificationPlan: backup', () => {
  const stale = (patch: Partial<Milestone> = {}) =>
    milestone('backup_stale', { key: 'backup_stale', sinceDay: utcDay(2026, 8, 1), ...patch });
  const BACKUP_ONLY: NotificationPrefs = { ...NOTHING, backup: true };

  it('plans one backup reminder tomorrow only while backup_stale is present, unmuted and enabled', () => {
    const m = stale();
    expect(plan({ milestones: [m], prefs: BACKUP_ONLY })).toEqual([
      {
        id: notificationId('backup', 'backup'),
        kind: 'backup',
        at: at(2026, 10, 18, 9, 0),
        title: { key: 'notifications.backup.title' },
        body: { key: 'notifications.backup.stale' },
      },
    ]);
    // No backup_stale milestone: the backup is fresh
    expect(plan({ milestones: [], prefs: BACKUP_ONLY })).toEqual([]);
    // Muted in Připomínky
    expect(plan({ milestones: [m], mutedKinds: ['backup_stale'], prefs: BACKUP_ONLY })).toEqual([]);
    // Switched off in Oznámení
    expect(plan({ milestones: [m], prefs: { ...BACKUP_ONLY, backup: false } })).toEqual([]);
  });

  it('says there is no backup at all when the milestone has no last day', () => {
    const [item] = plan({ milestones: [stale({ sinceDay: null })], prefs: BACKUP_ONLY });
    expect(item.body).toEqual({ key: 'notifications.backup.never' });
  });

  it('tomorrow crosses month and year ends', () => {
    const m = stale();
    expect(plan({ now: at(2026, 10, 31, 20), milestones: [m], prefs: BACKUP_ONLY })[0].at).toEqual(
      at(2026, 11, 1, 9, 0)
    );
    expect(plan({ now: at(2026, 12, 31, 20), milestones: [m], prefs: BACKUP_ONLY })[0].at).toEqual(
      at(2027, 1, 1, 9, 0)
    );
  });
});

describe('notificationId', () => {
  it('is stable and never 0', () => {
    expect(notificationId('upkeep', 'test')).toBe(notificationId('upkeep', 'test'));
    // FNV-1a of "upkeep:test", cut to 31 bits (worked out outside the code)
    expect(notificationId('upkeep', 'test')).toBe(1544772105);
    expect(notificationId('upkeep', 'upkeep')).toBe(385850613);
    expect(notificationId('backup', 'backup')).toBe(1781085509);
    expect(notificationId('deadline14', 'insurance_end:policy-1')).toBe(609058710);
  });

  it('differs by kind and discriminator and fits a signed 32-bit integer', () => {
    const ids = [
      notificationId('deadline14', 'a'),
      notificationId('deadline1', 'a'),
      notificationId('deadline14', 'b'),
      notificationId('upkeep', 'a'),
      notificationId('backup', 'a'),
    ];
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) {
      expect(Number.isInteger(id)).toBe(true);
      expect(id).toBeGreaterThan(0);
      expect(id).toBeLessThanOrEqual(2 ** 31 - 1);
    }
  });
});

describe('buildNotificationPlan: cap', () => {
  it('caps the plan at MAX_PENDING keeping the earliest, with distinct non-zero ids', () => {
    // 31 deadlines on consecutive days = 62 items, all ahead of now
    const milestones = Array.from({ length: 31 }, (_, i) =>
      milestone('insurance_end', {
        key: `insurance_end:policy-${i}`,
        dueDay: utcDay(2027, 3, 1 + i),
      })
    );
    const result = plan({ milestones });
    expect(result).toHaveLength(MAX_PENDING);
    expect(new Set(result.map((p) => p.id)).size).toBe(MAX_PENDING);
    expect(result.every((p) => p.id > 0)).toBe(true);
    // Earliest first
    for (let i = 1; i < result.length; i++) {
      expect(isAfter(result[i - 1].at, result[i].at)).toBe(false);
    }
    // The two latest items (T-1 of the last two policies) are the ones cut
    const ids = new Set(result.map((p) => p.id));
    expect(ids.has(notificationId('deadline1', 'insurance_end:policy-30'))).toBe(false);
    expect(ids.has(notificationId('deadline1', 'insurance_end:policy-29'))).toBe(false);
    expect(ids.has(notificationId('deadline1', 'insurance_end:policy-28'))).toBe(true);
    expect(ids.has(notificationId('deadline14', 'insurance_end:policy-0'))).toBe(true);
  });
});

describe('planSignature', () => {
  const m = milestone('insurance_end', { key: 'insurance_end:p', dueDay: utcDay(2027, 3, 20) });

  it('is the same for the same plan and different when an id or a date changes', () => {
    const base = planSignature(plan({ milestones: [m] }));
    expect(planSignature(plan({ milestones: [m] }))).toBe(base);
    expect(planSignature(plan({ milestones: [{ ...m, dueDay: utcDay(2027, 3, 21) }] }))).not.toBe(
      base
    );
    expect(planSignature(plan({ milestones: [{ ...m, key: 'insurance_end:q' }] }))).not.toBe(base);
    expect(planSignature(plan({ milestones: [] }))).not.toBe(base);
  });

  it('is empty for an empty plan', () => {
    expect(planSignature([])).toBe('');
  });
});
