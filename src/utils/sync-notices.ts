import type {
  SyncDevice,
  SyncNewDevice,
  SyncRemovalNotDone,
  SyncRemovalNotice,
  SyncRemovedNotice,
} from '@shared/schema';

/**
 * The `deviceSync` key of a removal notice's sentence, and whether the "also made a new recovery
 * kit" sentence follows: a change that removed nobody and made a kit reads as the new-kit sentence.
 */
export function removalNoticeText(notice: Pick<SyncRemovalNotice, 'removed' | 'kitReplaced'>): {
  key: 'notice.removal.removed' | 'notice.removal.newKit';
  kitToo: boolean;
} {
  if (notice.removed.length === 0 && notice.kitReplaced) {
    return { key: 'notice.removal.newKit', kitToo: false };
  }
  return { key: 'notice.removal.removed', kitToo: notice.kitReplaced };
}

/**
 * The `deviceSync` key of a removal that did not happen: a new recovery kit that never started
 * working, the other device that removed them meanwhile, the one that changed the list meanwhile,
 * or the sync server's refusal (`by` is `null`).
 */
export function removalNotDoneText(
  notice: Pick<SyncRemovalNotDone, 'by' | 'alreadyRemoved' | 'kitReplaced'>
):
  | 'notice.notDone.kit'
  | 'notice.notDone.alreadyRemoved'
  | 'notice.notDone.changed'
  | 'notice.notDone.refused' {
  if (notice.kitReplaced) return 'notice.notDone.kit';
  if (notice.by === null) return 'notice.notDone.refused';
  return notice.alreadyRemoved ? 'notice.notDone.alreadyRemoved' : 'notice.notDone.changed';
}

/** How a new device is announced: joined with the kit, added by a device that left (a late vouch), or added by one. */
export type NewDeviceKind = 'kit' | 'lateVouch' | 'added';

export function newDeviceKind(device: Pick<SyncNewDevice, 'viaKit' | 'lateVouch'>): NewDeviceKind {
  if (device.viaKit) return 'kit';
  return device.lateVouch ? 'lateVouch' : 'added';
}

/**
 * The devices a Kit joiner's hold names (`afterKitJoin.names`) that are still on the list: other
 * devices that are not former, matched by name. Removing them lifts the hold.
 */
export function afterKitJoinDevices(
  names: readonly string[],
  devices: readonly SyncDevice[] | undefined
): SyncDevice[] {
  const wanted = new Set(names);
  return (devices ?? []).filter(
    (device) => !device.thisDevice && device.status !== 'former' && wanted.has(device.name)
  );
}

/** What the removed-device notice says: its title and body keys, and whether the unsent count follows. */
export function removedNoticeKeys(notice: Pick<SyncRemovedNotice, 'left' | 'unsent'>): {
  titleKey: 'removed.title' | 'removed.titleLeft';
  bodyKey: 'removed.body' | 'removed.bodyLeft';
  showUnsent: boolean;
} {
  return notice.left
    ? { titleKey: 'removed.titleLeft', bodyKey: 'removed.bodyLeft', showUnsent: notice.unsent > 0 }
    : { titleKey: 'removed.title', bodyKey: 'removed.body', showUnsent: notice.unsent > 0 };
}
