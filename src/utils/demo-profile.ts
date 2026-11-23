/**
 * The demo profile's first run (ADR 0037, plan `docs/plans/2026-11-23-demo-profile-app.md`): which
 * first-run screen the auth page shows, and how "last opened" reads. Pure, so the decisions most
 * likely to regress are tested without React.
 */
import type { OnboardingPath } from '@/utils/onboarding-steps';

/** The persona every demo shows (ADR 0037, `services::demo::seed`): the same in every language. */
export const DEMO_PERSONA = { name: 'Alex Meyer', initials: 'AM' } as const;

/** The first-run screens of the auth page, in the order a new person meets them. */
export type FirstRunScreen = 'intro' | 'choice' | 'wizard';

/**
 * How the wizard was entered: from the choice screen (its back control returns there) or from the
 * demo (its back control reopens the demo), on the new-profile path or straight on the join path.
 */
export interface WizardEntry {
  from: 'choice' | 'demo';
  path: Extract<OnboardingPath, 'new' | 'join'>;
}

export interface FirstRunInput {
  /** The desktop shell; the phone keeps today's intro → wizard and has no demo. */
  desktop: boolean;
  /** The brand intro ran (or was skipped) in this visit of the auth page. */
  introDone: boolean;
  /** A demo is stored: a returning person, who starts on the choice screen without the intro. */
  demoExists: boolean;
  /** Set once the wizard was entered; null on the choice screen. */
  wizardEntry: WizardEntry | null;
}

/**
 * The screen to show while no real profile exists.
 *
 * - Phone: the intro, then the wizard (no choice, no demo).
 * - Desktop: an entered wizard wins (entered from the demo it skips the intro); otherwise the
 *   intro plays once for a new person and the choice screen follows. A person with a stored demo
 *   (the next launch, or right after leaving the demo) starts on the choice screen.
 */
export function firstRunScreen({
  desktop,
  introDone,
  demoExists,
  wizardEntry,
}: FirstRunInput): FirstRunScreen {
  if (!desktop) return introDone ? 'wizard' : 'intro';
  if (wizardEntry) return 'wizard';
  return introDone || demoExists ? 'choice' : 'intro';
}

/** How a past instant relates to today, in the local calendar (the lock screen's "Zamčeno" rule). */
export type DayRelation = 'today' | 'yesterday' | 'earlier';

export function dayRelation(unixSeconds: number, now: Date): DayRelation {
  const day = new Date(unixSeconds * 1000).toDateString();
  if (day === now.toDateString()) return 'today';
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
  return day === yesterday.toDateString() ? 'yesterday' : 'earlier';
}
