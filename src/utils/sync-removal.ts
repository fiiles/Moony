import type {
  RecoveryKit,
  RemovalOutcome,
  RemovalPlan,
  SyncCheckReason,
  SyncStatus,
} from '@shared/schema';

/** Devices can be removed from this sync now: phase 5's vault, sync on, no key change of ours still waiting. */
export function removalAvailable(status: SyncStatus | undefined): boolean {
  return status?.removalSupported === true && !status.removalPending;
}

/**
 * The `deviceSync` key of the plan's recovery-kit line, or `null` when the dialog says nothing:
 * the kit stays and is not involved (`no`), or a new one was asked for (`asked`: the new-kit dialog
 * has its own words). `kitNotUpdated` (no kit involved at all) has its own line.
 */
export function kitLineKey(
  plan: Pick<RemovalPlan, 'replacesKit' | 'kitNotUpdated'>
): string | null {
  switch (plan.replacesKit) {
    case 'creatorRemoved':
      return 'remove.kit.creatorRemoved';
    case 'cloud':
      return 'remove.kit.cloud';
    case 'missing':
      return 'remove.kit.missing';
    case 'asked':
      return null;
    case 'no':
      return plan.kitNotUpdated ? 'remove.kit.notUpdated' : null;
  }
}

/** Whether the plan makes a new recovery kit (so the outcome carries one and the Kit step follows). */
export function planMakesKit(plan: Pick<RemovalPlan, 'replacesKit'>): boolean {
  return plan.replacesKit !== 'no';
}

/** The short reason a device drops out of sync with the removal (`deviceSync` key). */
export function dropReasonKey(reason: SyncCheckReason): string {
  return `remove.dropReason.${reason}`;
}

/** "Je moje" inline in the list of devices that drop out: only a late vouch can be kept. */
export function canKeepDropping(reason: SyncCheckReason): boolean {
  return reason === 'lateVouch';
}

/** Ticks or unticks `id`, keeping the order the ticks were made in. */
export function toggleId(ids: readonly string[], id: string): string[] {
  return ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id];
}

/**
 * The ticks that still name a device the (reloaded) plan lists as added by the removed ones; a
 * stale tick must not be sent. The plan keeps a ticked device listed, so a tick survives a reload.
 */
export function pruneAlsoRemove(
  alsoRemove: readonly string[],
  plan: RemovalPlan | undefined
): string[] {
  if (!plan) return [...alsoRemove];
  const offered = new Set(plan.addedByRemoving.map((device) => device.id));
  return alsoRemove.filter((id) => offered.has(id));
}

/** The name the dialog's title uses: the first device asked for, as the plan names it. */
export function removalTitleName(
  plan: RemovalPlan | undefined,
  deviceIds: readonly string[]
): string | null {
  const first = plan?.removing.find((device) => deviceIds.includes(device.id));
  return first?.name ?? null;
}

/**
 * The arguments of `sync_remove_devices` for a plan that was confirmed (`password` is added by the
 * caller). `newKit` is the new-kit dialog's: no device asked for, a new recovery kit.
 */
export function removalRequest(
  plan: RemovalPlan,
  deviceIds: readonly string[],
  alsoRemove: readonly string[],
  newKit = false
) {
  return {
    deviceIds: [...deviceIds],
    alsoRemove: pruneAlsoRemove(alsoRemove, plan),
    newKit,
    consent: plan.consent,
  };
}

/** What the dialog does with an outcome. */
export interface OutcomeView {
  /** The toast: removed; the sync server is out of reach and it finishes by itself; someone else removed them; this device was removed. */
  toast: 'removed' | 'pending' | 'alreadyRemoved' | 'removedHere';
  /** `by` for `alreadyRemoved`. */
  by: string | null;
  /** The new recovery kit to show once (the Kit step), when the outcome carries one. */
  kit: RecoveryKit | null;
  /** The Kit step says the kit starts working only once the removal finishes. */
  kitPending: boolean;
}

/** `done` and `pending` show the new kit when they carry one; the other two never do. */
export function outcomeView(outcome: RemovalOutcome): OutcomeView {
  switch (outcome.kind) {
    case 'done':
      return { toast: 'removed', by: null, kit: outcome.kit, kitPending: false };
    case 'pending':
      return { toast: 'pending', by: null, kit: outcome.kit, kitPending: true };
    case 'alreadyRemoved':
      return { toast: 'alreadyRemoved', by: outcome.by, kit: null, kitPending: false };
    case 'removedHere':
      return { toast: 'removedHere', by: outcome.by, kit: null, kitPending: false };
  }
}

/** The backend's answer when the plan the person confirmed no longer holds: reload it and ask again. */
export const PLAN_CHANGED_KEY = 'validation.syncRemovalPlanChanged';
