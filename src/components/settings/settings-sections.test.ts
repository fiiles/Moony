import { describe, expect, it } from 'vitest';
import {
  SETTINGS_SECTIONS,
  isLockedSection,
  resolveSettingsSection,
  visibleSettingsSections,
} from './settings-sections';

describe('SETTINGS_SECTIONS', () => {
  it('lists the sub-pages in display order with unique paths', () => {
    expect(SETTINGS_SECTIONS.map((s) => s.id)).toEqual([
      'general',
      'security',
      'sync',
      'data',
      'integrations',
      'categorization',
      'account',
    ]);
    expect(SETTINGS_SECTIONS.map((s) => s.path)).toEqual([
      '/settings',
      '/settings/security',
      '/settings/sync',
      '/settings/data',
      '/settings/integrations',
      '/settings/categorization',
      '/settings/account',
    ]);
  });

  it('never shadows the separate categorization-rules page', () => {
    expect(SETTINGS_SECTIONS.some((s) => s.path === '/settings/categorization-rules')).toBe(false);
  });
});

describe('resolveSettingsSection', () => {
  it('maps every section path to its section', () => {
    for (const section of SETTINGS_SECTIONS) {
      expect(resolveSettingsSection(section.path, true)).toBe(section.id);
    }
  });

  it('treats /settings as General', () => {
    expect(resolveSettingsSection('/settings')).toBe('general');
    expect(resolveSettingsSection('/settings/')).toBe('general');
  });

  it('falls back to General for unknown or nested paths instead of a 404', () => {
    expect(resolveSettingsSection('/settings/nope')).toBe('general');
    expect(resolveSettingsSection('/settings/security/extra')).toBe('general');
    expect(resolveSettingsSection('/settings/DATA')).toBe('general');
    expect(resolveSettingsSection('/')).toBe('general');
  });

  it('ignores a trailing slash, query and hash', () => {
    expect(resolveSettingsSection('/settings/integrations/')).toBe('integrations');
    expect(resolveSettingsSection('/settings/integrations#mcp')).toBe('integrations');
    expect(resolveSettingsSection('/settings/data?x=1')).toBe('data');
  });
});

describe('sync section', () => {
  it('lists the sync section only when the feature is on, before Data and backups', () => {
    expect(visibleSettingsSections(false).map((s) => s.id)).toEqual([
      'general',
      'security',
      'data',
      'integrations',
      'categorization',
      'account',
    ]);
    const on = visibleSettingsSections(true).map((s) => s.id);
    expect(on).toEqual([
      'general',
      'security',
      'sync',
      'data',
      'integrations',
      'categorization',
      'account',
    ]);
  });

  it('resolves /settings/sync to general while the feature is off', () => {
    expect(resolveSettingsSection('/settings/sync')).toBe('general');
    expect(resolveSettingsSection('/settings/sync', false)).toBe('general');
    expect(resolveSettingsSection('/settings/sync', true)).toBe('sync');
  });
});

describe('the demo profile (ADR 0037)', () => {
  it('lists Ukázka first, the open sections, then the locked ones, and no Account', () => {
    const sections = visibleSettingsSections(true, true);
    expect(sections.map((s) => s.id)).toEqual([
      'demo',
      'general',
      'categorization',
      'security',
      'sync',
      'data',
      'integrations',
    ]);
    expect(sections.filter((s) => s.locked).map((s) => s.id)).toEqual([
      'security',
      'sync',
      'data',
      'integrations',
    ]);
  });

  it('leaves Sync out while the feature is off', () => {
    expect(visibleSettingsSections(false, true).map((s) => s.id)).not.toContain('sync');
  });

  it('lists Ukázka only in the demo', () => {
    expect(visibleSettingsSections(true).map((s) => s.id)).not.toContain('demo');
    expect(resolveSettingsSection('/settings/demo', true)).toBe('general');
    expect(resolveSettingsSection('/settings/demo', true, true)).toBe('demo');
  });

  it('sends a link to Account back to General in the demo', () => {
    expect(resolveSettingsSection('/settings/account', true, true)).toBe('general');
    expect(resolveSettingsSection('/settings/account', true, false)).toBe('account');
  });

  it('resolves a locked section to itself, so its page can say why it is locked', () => {
    expect(resolveSettingsSection('/settings/data', true, true)).toBe('data');
    expect(isLockedSection('data', true)).toBe(true);
    expect(isLockedSection('data', false)).toBe(false);
    expect(isLockedSection('general', true)).toBe(false);
    expect(isLockedSection('categorization', true)).toBe(false);
  });
});
