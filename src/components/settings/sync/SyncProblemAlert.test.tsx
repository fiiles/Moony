import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import type { SyncStatus } from '@shared/schema';
import { SyncProblemAlert } from './SyncProblemAlert';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      `${key}|${Object.values(values ?? {}).join('|')}`,
  }),
}));
vi.mock('@/lib/use-format', () => ({
  useFormat: () => ({ date: () => 'D', locale: 'en' }),
}));
vi.mock('@/hooks/use-device-sync', () => ({ useHeldGaps: () => ({ data: undefined }) }));
vi.mock('@/hooks/use-device-sync-mutations', () => ({
  useDeviceSyncMutations: () => ({ syncNow: { mutateAsync: vi.fn(), isPending: false } }),
}));
vi.mock('@/components/settings/sync/SyncRenewDialog', () => ({ SyncRenewDialog: () => null }));
vi.mock('@/components/settings/sync/SyncRepairDialogs', () => ({
  RepairDeviceDialog: () => null,
  ResyncDeviceDialog: () => null,
}));
vi.mock('@/components/settings/sync/RejoinDialog', () => ({ RejoinDialog: () => null }));
vi.mock('@/components/settings/sync/TurnOffSyncDialog', () => ({ TurnOffSyncDialog: () => null }));
vi.mock('@/components/settings/sync/SkipMissingChangeDialog', () => ({
  SkipMissingChangeDialog: () => null,
}));

const DAY = 86_400;
const now = Math.floor(Date.now() / 1000);

function status(over: Partial<SyncStatus>): SyncStatus {
  return {
    phase: 'waiting',
    problem: 'keyChangeWaiting',
    lastSuccessAt: now - 3 * DAY,
    waitingForRemover: { by: 'Mac', since: now - 2 * DAY },
    rejoinBy: null,
    ...over,
  } as SyncStatus;
}

const render = (s: SyncStatus) =>
  renderToStaticMarkup(<SyncProblemAlert status={s} onRejoin={() => undefined} />);

describe('SyncProblemAlert, the waiting state', () => {
  it('shows the title and the body and offers no button', () => {
    for (const problem of ['keyChangeWaiting', 'keyRotated'] as const) {
      const html = render(status({ problem }));
      expect(html).toContain('problem.waiting.title|Mac');
      expect(html).toContain('problem.waiting.body|Mac');
      expect(html).not.toContain('<button');
      expect(html).not.toContain('problem.rejoin.button');
    }
  });

  it('still offers "Connect again" in the rejoin state', () => {
    const html = render(
      status({ problem: 'keyNotGiven', waitingForRemover: null, rejoinBy: 'Tablet' })
    );
    expect(html).toContain('problem.rejoin.button');
  });
});
