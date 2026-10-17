/**
 * The state sub-lines of Nastavení's root (spec §8.9): what Zabezpečení says about the phone in a
 * few words ("Odemykání obličejem · zámek po 1 minutě"). Pure: the builders return `TextRef`s
 * (a `mobile` namespace key and its values) that the page translates with `renderTextRef`, so the
 * wording stays in the locale files and the rules stay testable.
 */
import type { TFunction } from 'i18next';
import type { BiometryType } from '@shared/schema';
import type { BackgroundLockMinutes } from '@/mobile/lib/background-lock';
import { biometryKind, type BiometryKind } from '@/mobile/lib/biometrics';

/** A `mobile` namespace key with its `{{values}}`. */
export interface TextRef {
  key: string;
  params?: Record<string, string | number>;
}

const METHOD_KEY: Record<BiometryKind, string> = {
  face: 'settings.security.methodFace',
  touch: 'settings.security.methodTouch',
  generic: 'settings.security.methodGeneric',
};

/** The key that names how the phone is unlocked ("obličejem", "otiskem", "biometrií"). */
export function biometryMethodKey(type: BiometryType): string {
  return METHOD_KEY[biometryKind(type)];
}

/** The segment label of one background-lock choice: "Ihned", "1 min", "5 min", "15 min". */
export function lockAfterOption(minutes: BackgroundLockMinutes): TextRef {
  return minutes === 0
    ? { key: 'settings.security.optionNow' }
    : { key: 'settings.security.optionMinutes', params: { count: minutes } };
}

/**
 * Zabezpečení's sub-line on the root, as parts to join with " · ": how the phone unlocks, when it
 * locks after the app was left, and — only while it is on — that amounts start hidden.
 */
export function securitySummary(input: {
  enrolled: boolean;
  biometryType: BiometryType;
  lockMinutes: BackgroundLockMinutes;
  hideOnLaunch: boolean;
}): TextRef[] {
  const parts: TextRef[] = [
    input.enrolled
      ? {
          key: 'settings.security.biometricsOn',
          params: { method: biometryMethodKey(input.biometryType) },
        }
      : { key: 'settings.security.biometricsOff' },
    input.lockMinutes === 0
      ? { key: 'settings.security.lockNow' }
      : { key: 'settings.security.lockAfter', params: { count: input.lockMinutes } },
  ];
  if (input.hideOnLaunch) parts.push({ key: 'settings.security.hideOnLaunchShort' });
  return parts;
}

/**
 * Says a `TextRef` with the `mobile` translator. A `method` value is itself a key (it names the
 * biometry and has to be in the page's language before it goes into the sentence), the other
 * values are numbers or words already.
 */
export function renderTextRef(t: TFunction, ref: TextRef): string {
  if (!ref.params) return t(ref.key);
  const params = { ...ref.params };
  if (typeof params.method === 'string') params.method = t(params.method);
  return t(ref.key, params);
}
