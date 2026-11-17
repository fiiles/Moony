import type { SyncDevice } from '@shared/schema';

const KNOWN_PLATFORMS = new Set(['macos', 'windows', 'linux', 'ios', 'android']);

/** The i18n key of a device's platform label; unknown platforms read as "other". */
export function devicePlatformKey(platform: string): string {
  return KNOWN_PLATFORMS.has(platform) ? `devices.platform.${platform}` : 'devices.platform.other';
}

/** The `setup.defaultName` key for this desktop, read from the user agent. */
export function defaultDeviceKey(): 'macos' | 'windows' | 'linux' {
  const ua = navigator.userAgent;
  if (/Windows/i.test(ua)) return 'windows';
  if (/Linux|X11/i.test(ua) && !/Mac/i.test(ua)) return 'linux';
  return 'macos';
}

/**
 * A device not heard from for more days than this is marked as quiet in the phone's device list.
 * The desktop list reads the backend's `inactiveDays` instead; the phone does so with Task 30,
 * which then removes this.
 */
export const QUIET_AFTER_DAYS = 30;

const SECONDS_PER_DAY = 86_400;

/**
 * Whole days since a device was last heard from (both unix seconds), when that is more than
 * QUIET_AFTER_DAYS; null otherwise or when unknown.
 */
export function quietDays(lastActiveAt: number | null, now: number): number | null {
  if (lastActiveAt === null) return null;
  const days = Math.floor((now - lastActiveAt) / SECONDS_PER_DAY);
  return days > QUIET_AFTER_DAYS ? days : null;
}

/** The device list in its three groups, each in the order the backend gave (this device first among the active). */
export interface DeviceGroups {
  active: SyncDevice[];
  /** "Zkontrolujte": they get no key at the next key change unless kept. */
  check: SyncDevice[];
  /** "Dřívější zařízení": left or removed; the backend lists them for 30 days only. */
  former: SyncDevice[];
}

/**
 * Splits the list by `status`. Nothing is filtered here: a former device older than 30 days is
 * already absent from the backend's list. This device always leads the active group.
 */
export function groupDevices(devices: readonly SyncDevice[] | undefined): DeviceGroups {
  const list = devices ?? [];
  const active = list.filter((device) => device.status === 'active');
  return {
    active: [...active.filter((d) => d.thisDevice), ...active.filter((d) => !d.thisDevice)],
    check: list.filter((device) => device.status === 'check'),
    former: list.filter((device) => device.status === 'former'),
  };
}

/** A device the person can remove: another device that is on the list, active or "Zkontrolujte". */
export function canRemoveDevice(device: SyncDevice): boolean {
  return !device.thisDevice && device.status !== 'former' && !device.removalPending;
}

/** "Je moje" is offered on a late-vouch row only; it helps only until the next key change (R-77a). */
export function canKeepDevice(device: SyncDevice): boolean {
  return device.status === 'check' && device.checkReason === 'lateVouch';
}

/** The row's name, with " (kopie)" after a superseded ID that still writes. */
export function deviceLabelKey(device: SyncDevice): 'devices.copyName' | null {
  return device.status === 'check' && device.checkReason === 'copy' ? 'devices.copyName' : null;
}

/**
 * The `deviceSync` key of why a row says "Zkontrolujte"; `{{by}}` is the name of `addedBy`. A late
 * vouch with no known adder has its own sentence, and the sentence fits an adder that is itself
 * "Zkontrolujte".
 */
export function checkReasonKey(device: SyncDevice): string | null {
  if (device.status !== 'check' || device.checkReason === null) return null;
  if (device.checkReason === 'lateVouch') {
    return device.addedBy === null ? 'devices.check.lateVouchNoName' : 'devices.check.lateVouch';
  }
  return `devices.check.${device.checkReason}`;
}

/**
 * The `deviceSync` key of the details line saying how the device came to the list (values:
 * `{{date}}` from `joinedAt`, `{{name}}` from `addedBy`); `null` when this device does not know.
 */
export function joinedViaKey(device: SyncDevice): string | null {
  switch (device.joinedVia) {
    case 'created':
      return 'devices.via.created';
    case 'paired':
      return device.addedBy === null ? 'devices.via.paired' : 'devices.via.pairedBy';
    case 'recoveryKit':
      return 'devices.via.recoveryKit';
    case 'rejoined':
      return 'devices.via.rejoined';
    case 'unknown':
      return null;
  }
}

/**
 * The `deviceSync` key of a former row's line: "odpojeno {date}" or "odebráno {date} zařízením
 * {by}", each part only when known (`{{date}}` from `stateAt`, `{{by}}` from `stateBy`).
 */
export function formerLineKey(device: SyncDevice): string {
  if (device.formerReason === 'left') {
    return device.stateAt === null ? 'devices.former.left' : 'devices.former.leftOn';
  }
  const on = device.stateAt === null ? '' : 'On';
  const by = device.stateBy === null ? '' : 'By';
  return `devices.former.removed${on}${by}`;
}

/** The days an active other device has been silent, when the backend marks it inactive (30 or more); else `null`. */
export function inactiveDays(device: SyncDevice): number | null {
  return device.status === 'active' && !device.thisDevice ? device.inactiveDays : null;
}

/** The names of the account's other devices, in list order, trimmed, empty names left out. */
export function otherDeviceNames(devices: readonly SyncDevice[] | undefined): string[] {
  return (devices ?? [])
    .filter((device) => !device.thisDevice)
    .map((device) => device.name.trim())
    .filter((name) => name !== '');
}
