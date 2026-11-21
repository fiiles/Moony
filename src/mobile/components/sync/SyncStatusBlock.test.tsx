import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import type { SyncStatus } from '@shared/schema';
import { SyncStatusBlock } from './SyncStatusBlock';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      `${key}|${Object.values(values ?? {}).join('|')}`,
  }),
}));
vi.mock('@/lib/use-format', () => ({
  useFormat: () => ({ date: () => 'D', dateTime: () => 'DT', locale: 'en' }),
}));
vi.mock('@/hooks/use-attachments-status-text', () => ({
  useAttachmentsStatusText: () => ({ waiting: null, failedUpload: null, problem: null }),
}));
vi.mock('@/hooks/use-device-sync', () => ({ useHeldGaps: () => ({ data: undefined }) }));
vi.mock('@/hooks/use-device-sync-mutations', () => ({
  useDeviceSyncMutations: () => ({
    syncNow: { mutateAsync: vi.fn(), isPending: false },
    downloadEverything: { mutateAsync: vi.fn(), isPending: false },
  }),
}));
vi.mock('@/mobile/components/sync/SyncNotices', () => ({ SyncNotices: () => null }));
// The alert's own actions, as labels, apart from the card's "Sync now" button
vi.mock('@/mobile/components/InlineAlert', () => ({
  InlineAlert: (props: {
    title?: string;
    action?: { label: string } | readonly { label: string }[];
    children: React.ReactNode;
  }) => {
    const actions =
      props.action === undefined ? [] : 'label' in props.action ? [props.action] : props.action;
    return (
      <div data-testid="alert">
        {props.title}
        {props.children}
        {actions.map((a) => (
          <span key={a.label} data-action>
            {a.label}
          </span>
        ))}
      </div>
    );
  },
}));

const DAY = 86_400;
const now = Math.floor(Date.now() / 1000);

function status(over: Partial<SyncStatus>): SyncStatus {
  return {
    phase: 'waiting',
    problem: 'keyChangeWaiting',
    lastSuccessAt: now - 3 * DAY,
    pendingChanges: 0,
    serverAddress: null,
    serverInsecure: false,
    clockAheadUntil: null,
    attachments: {
      waitingUpload: 0,
      waitingUploadBytes: 0,
      notHere: 0,
      notOnServer: 0,
      failed: 0,
      notOnDevice: 0,
      failedUpload: 0,
      problem: null,
    },
    waitingForRemover: { by: 'Mac', since: now - 2 * DAY },
    rejoinBy: null,
    ...over,
  } as SyncStatus;
}

const render = (s: SyncStatus) =>
  renderToStaticMarkup(
    <SyncStatusBlock
      status={s}
      offerResync={false}
      onRepair={() => undefined}
      onResync={() => undefined}
      onTurnOff={() => undefined}
      onRejoin={() => undefined}
      onSkipMissing={() => undefined}
    />
  );

describe('SyncStatusBlock, the waiting state', () => {
  it('shows the title and the body and offers no action', () => {
    for (const problem of ['keyChangeWaiting', 'keyRotated'] as const) {
      const html = render(status({ problem }));
      expect(html).toContain('problem.waiting.title|Mac');
      expect(html).toContain('problem.waiting.body|Mac');
      expect(html).not.toContain('data-action');
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
