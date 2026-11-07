/**
 * Where a tapped notification leads (spec §6, §9; audit C4). Every planned notification carries
 * its screen (`PlannedNotification.path`): in its `extra`, which the Android plugin hands back on a
 * tap, and in the scheduler's map from id to path, because the iOS plugin hands back the id but no
 * `extra`. The tapped target waits in a slot until the app is unlocked; whoever opens it takes it,
 * once. Only a path of the shell is ever accepted (`isShellPath`).
 */
import { isShellPath } from '@/mobile/shell/routes';

/** What the plugin's `onAction` reports, as far as it is read here. */
export interface NotificationTap {
  actionId?: string;
  notification?: { id?: number; extra?: Record<string, unknown> | null } | null;
}

/** The screen a tap opens, or `null` (another action, an unknown notification, a foreign path). */
export function tapTarget(tap: NotificationTap, paths: ReadonlyMap<number, string>): string | null {
  if (tap.actionId !== 'tap' || !tap.notification) return null;
  const { id, extra } = tap.notification;
  const fromExtra = extra?.path;
  if (isShellPath(fromExtra)) return fromExtra;
  const fromMap = typeof id === 'number' ? paths.get(id) : undefined;
  return isShellPath(fromMap) ? fromMap : null;
}

export interface TargetSlot {
  /** A tap's target arrives; it replaces one not yet opened. */
  offer(path: string): void;
  /** The waiting target, without taking it. */
  peek(): string | null;
  /** The waiting target, once. */
  take(): string | null;
  subscribe(listener: () => void): () => void;
}

export function createTargetSlot(): TargetSlot {
  let path: string | null = null;
  let listeners: (() => void)[] = [];
  const emit = () => listeners.forEach((listener) => listener());
  return {
    offer(next) {
      if (!isShellPath(next)) return;
      path = next;
      emit();
    },
    peek: () => path,
    take() {
      const taken = path;
      path = null;
      if (taken !== null) emit();
      return taken;
    },
    subscribe(listener) {
      listeners = [...listeners, listener];
      return () => {
        listeners = listeners.filter((l) => l !== listener);
      };
    },
  };
}

/** The app's one slot (`NotificationTargetHost` fills it; the host or the unlock opens it). */
export const notificationTarget = createTargetSlot();

/** The screens of the notifications scheduled by this run of the app, by id (iOS taps). */
export const notificationPaths = new Map<number, string>();
