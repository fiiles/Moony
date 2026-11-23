/**
 * Settings sub-pages.
 * Pure data and path resolution; icons and components live in SettingsLayout.
 */

export type SettingsSectionId =
  'demo' | 'general' | 'security' | 'data' | 'integrations' | 'sync' | 'categorization' | 'account';

export interface SettingsSection {
  id: SettingsSectionId;
  path: string;
  /** Key in the `settings` namespace. */
  labelKey: string;
  /** In the demo profile: listed, but its page only says it comes with an own profile. */
  locked?: boolean;
}

export const SETTINGS_SECTIONS: readonly SettingsSection[] = [
  { id: 'general', path: '/settings', labelKey: 'sections.general' },
  { id: 'security', path: '/settings/security', labelKey: 'sections.security' },
  // Sync before Data and backups: the first way to look after the data (owner, 2026-11-17)
  { id: 'sync', path: '/settings/sync', labelKey: 'sections.sync' },
  { id: 'data', path: '/settings/data', labelKey: 'sections.data' },
  { id: 'integrations', path: '/settings/integrations', labelKey: 'sections.integrations' },
  { id: 'categorization', path: '/settings/categorization', labelKey: 'sections.categorization' },
  { id: 'account', path: '/settings/account', labelKey: 'sections.account' },
];

/** The demo profile's own section (ADR 0037), first in its list. */
export const DEMO_SECTION: SettingsSection = {
  id: 'demo',
  path: '/settings/demo',
  labelKey: 'sections.demo',
};

/**
 * Sections the demo shows but locks: what needs a real profile (password, backups, sync, the MCP
 * server), whose actions the backend refuses in the demo.
 */
export const LOCKED_IN_DEMO: readonly SettingsSectionId[] = [
  'security',
  'sync',
  'data',
  'integrations',
];

/**
 * The sections to list: Sync only while the feature flag is on (it is hidden until release). In the
 * demo: Ukázka first, the open sections (General, Categorization), then the locked ones; Account
 * (deleting the data) is not listed.
 */
export function visibleSettingsSections(
  syncFeature: boolean,
  demo = false
): readonly SettingsSection[] {
  const sections = syncFeature
    ? SETTINGS_SECTIONS
    : SETTINGS_SECTIONS.filter((s) => s.id !== 'sync');
  if (!demo) return sections;
  const open = sections.filter((s) => s.id === 'general' || s.id === 'categorization');
  const locked = sections
    .filter((s) => LOCKED_IN_DEMO.includes(s.id))
    .map((s) => ({ ...s, locked: true }));
  return [DEMO_SECTION, ...open, ...locked];
}

/**
 * The section a location belongs to. Anything that is not an exact section
 * path (unknown or nested segments, or Sync while its feature is off) falls
 * back to General so a stale link never ends on the 404 page.
 */
export function resolveSettingsSection(
  location: string,
  syncFeature = false,
  demo = false
): SettingsSectionId {
  const pathname = location.split(/[?#]/)[0].replace(/\/+$/, '') || '/';
  return (
    visibleSettingsSections(syncFeature, demo).find((s) => s.path === pathname)?.id ?? 'general'
  );
}

/** Whether a section's page is replaced by the "comes with an own profile" panel. */
export function isLockedSection(id: SettingsSectionId, demo: boolean): boolean {
  return demo && LOCKED_IN_DEMO.includes(id);
}
