import { describe, expect, it } from 'vitest';
import { SETTINGS_SECTIONS, resolveSettingsSection } from './settings-sections';

describe('SETTINGS_SECTIONS', () => {
  it('lists the sub-pages in display order with unique paths', () => {
    expect(SETTINGS_SECTIONS.map((s) => s.id)).toEqual([
      'general',
      'security',
      'data',
      'integrations',
      'categorization',
      'account',
    ]);
    expect(SETTINGS_SECTIONS.map((s) => s.path)).toEqual([
      '/settings',
      '/settings/security',
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
      expect(resolveSettingsSection(section.path)).toBe(section.id);
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
