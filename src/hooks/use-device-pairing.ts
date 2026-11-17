import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { PairingView, ReplaceConsentView } from '@shared/schema';
import { ApiError } from '@/lib/api-error';
import { deviceSyncApi, portfolioApi } from '@/lib/tauri-api';
import { refreshAfterRemoteChanges } from '@/lib/device-sync-refresh';
import { dropQuery } from '@/lib/drop-query';
import {
  CODE_MISMATCH_KEY,
  joinFailureAction,
  pairingPollInterval,
  type JoinFailureAction,
  type JoinFailurePoll,
} from '@/utils/pairing-code';
import { REPLACE_DEVICE_MUTATION_KEY } from './use-device-sync-mutations';
import { useDeviceSyncFeature } from './use-device-sync';

const PAIRING_KEY = ['device-sync-pairing'] as const;

const CODE_INCOMPLETE_KEY = 'validation.syncPairingCodeIncomplete';

/** Confirm answers that leave the pairing going: the person types again. */
function isTypedCodeProblem(failure: unknown): boolean {
  return (
    failure instanceof ApiError &&
    (failure.key === CODE_MISMATCH_KEY || failure.key === CODE_INCOMPLETE_KEY)
  );
}

/**
 * The pairing flow on this device. The view lives under `['device-sync-pairing']`: start, scan and
 * confirm write their result there and a query polls the backend while a device is awaited.
 * Joining replaces local data, so it runs the full refresh and shares the busy-cover key.
 *
 * Passwords: `start` and `scan` keep their variables (the device password) in the mutation state,
 * so they are reset as soon as they succeed. A dialog that closes any other way (error, dismissed)
 * must call `cancel()` (resets both and clears the view) or `start.reset()` / `scan.reset()` itself.
 * `error` / `isError` report a failed poll (expired, cancelled by the other device, unreachable)
 * or a confirm that ended the pairing (a third wrong code, someone else answering, an old sync
 * server): the screen shows the message and offers to start again. A confirm that keeps the pairing
 * (a wrong or incomplete code with attempts left, a momentary failure) is `confirmError` and the
 * view is read again for `attemptsLeft`. Polling stops on an error and once `expiresAt` has passed. After a failed `join`,
 * `afterFailedJoin` asks the backend whether it kept the received session (`joinFailureAction`).
 */
export function useDevicePairing() {
  const queryClient = useQueryClient();
  const enabled = useDeviceSyncFeature();

  const {
    data: view,
    error,
    isError,
  } = useQuery<PairingView>({
    queryKey: PAIRING_KEY,
    queryFn: () => deviceSyncApi.pairingPoll(),
    // Only once a start or scan put a pairing there; the poll never starts one.
    enabled: enabled && queryClient.getQueryData(PAIRING_KEY) !== undefined,
    refetchInterval: (query) =>
      query.state.status === 'error' ||
      (query.state.data !== undefined && query.state.data.expiresAt * 1000 <= Date.now())
        ? false
        : pairingPollInterval(query.state.data),
    retry: false,
    // A finished or abandoned pairing must not keep its QR or code around.
    gcTime: 0,
  });

  const setView = (next: PairingView) => queryClient.setQueryData(PAIRING_KEY, next);
  // A confirm that ended the pairing: the backend gave the session up, so the view is dead.
  const [ended, setEnded] = useState<Error | null>(null);
  const setFreshView = (next: PairingView) => {
    setEnded(null);
    setView(next);
  };

  // gcTime 0 on start and scan: the device password is a mutation variable and must not linger.
  const start = useMutation({
    mutationFn: (opts: { password: string; relayUrl?: string; deviceName?: string }) =>
      deviceSyncApi.pairingStart(opts),
    gcTime: 0,
    onSuccess: setFreshView,
  });
  const scan = useMutation({
    mutationFn: ({ code, ...opts }: { code: string; password: string; deviceName?: string }) =>
      deviceSyncApi.pairingScan(code, opts),
    gcTime: 0,
    onSuccess: setFreshView,
  });
  // The password is no longer needed once the pairing is on its way.
  useEffect(() => {
    if (start.isSuccess) start.reset();
  }, [start, start.isSuccess]);
  useEffect(() => {
    if (scan.isSuccess) scan.reset();
  }, [scan, scan.isSuccess]);
  // The last confirm's answer when the pairing went on, held here because the mutation is reset
  // as soon as it settles: its variables are the typed digits and must not linger.
  const [confirmFailure, setConfirmFailure] = useState<Error | null>(null);
  const confirm = useMutation({
    // The holder's typed digits; the joiner's confirm has none.
    mutationFn: (typedCode?: string) => deviceSyncApi.pairingConfirm(typedCode),
    gcTime: 0,
    onMutate: () => setConfirmFailure(null),
    onSuccess: setView,
    onError: async (failure) => {
      setConfirmFailure(failure);
      // Polling is off while comparing: ask once, for `attemptsLeft`, and to learn whether the
      // pairing is still there.
      try {
        setView(await deviceSyncApi.pairingPoll());
      } catch (pollFailure) {
        setEnded((isTypedCodeProblem(failure) ? pollFailure : failure) as Error);
      }
    },
  });
  useEffect(() => {
    if (confirm.isSuccess || confirm.isError) confirm.reset();
  }, [confirm, confirm.isSuccess, confirm.isError]);
  // A join that succeeded ended the pairing by itself: nothing is left to cancel on unmount.
  const joined = useRef(false);
  const join = useMutation({
    mutationKey: REPLACE_DEVICE_MUTATION_KEY,
    mutationFn: (consent: ReplaceConsentView) => deviceSyncApi.pairingJoin(consent),
    gcTime: 0,
    onSuccess: async () => {
      joined.current = true;
      queryClient.removeQueries({ queryKey: PAIRING_KEY });
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['device-sync-status'] }),
        queryClient.invalidateQueries({ queryKey: ['device-sync-account'] }),
      ]);
      await refreshAfterRemoteChanges(queryClient, portfolioApi.recordSnapshot);
    },
  });

  /** A failed join: asks the backend whether the received session is still there to join again. */
  const afterFailedJoin = async (failure: unknown): Promise<JoinFailureAction> => {
    let poll: JoinFailurePoll;
    try {
      poll = { ok: true, stage: (await deviceSyncApi.pairingPoll()).stage };
    } catch (pollFailure) {
      poll = { ok: false, key: pollFailure instanceof ApiError ? pollFailure.key : undefined };
    }
    return joinFailureAction(poll, failure instanceof ApiError ? failure.key : undefined);
  };

  // The view, the ended state and the last confirm's answer go together once the backend gave the
  // pairing up, so the screen goes from the ended pairing straight to a fresh start (the password
  // step) and never shows the old view's code field. `dropQuery`, not `removeQueries`: the
  // mounted query would keep showing the old view.
  const cancel = async () => {
    start.reset();
    scan.reset();
    confirm.reset();
    try {
      await deviceSyncApi.pairingCancel();
    } finally {
      dropQuery(queryClient, PAIRING_KEY);
      setConfirmFailure(null);
      setEnded(null);
    }
  };

  // Whatever closes the dialog (auto-lock, another branch of the page, a route change) gives the
  // pairing up too. Cancelling is idempotent, so the dialogs' own close handlers may cancel first.
  const { reset: resetStart } = start;
  const { reset: resetScan } = scan;
  useEffect(
    () => () => {
      resetStart();
      resetScan();
      if (joined.current) return;
      queryClient.removeQueries({ queryKey: PAIRING_KEY });
      deviceSyncApi.pairingCancel().catch(() => undefined);
    },
    [queryClient, resetStart, resetScan]
  );

  return {
    view,
    error: ended ?? error,
    isError: isError || ended !== null,
    start,
    scan,
    confirm,
    confirmError: confirmFailure,
    cancel,
    join,
    afterFailedJoin,
  };
}
