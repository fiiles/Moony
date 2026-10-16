import { describe, expect, it } from 'vitest';
import { isMobileShell } from './platform';

describe('isMobileShell', () => {
  it('is true on the iOS and Android builds', () => {
    expect(isMobileShell({ TAURI_ENV_PLATFORM: 'ios' })).toBe(true);
    expect(isMobileShell({ TAURI_ENV_PLATFORM: 'android' })).toBe(true);
  });

  it('is false on desktop platforms and without any signal', () => {
    expect(isMobileShell({ TAURI_ENV_PLATFORM: 'macos' })).toBe(false);
    expect(isMobileShell({ TAURI_ENV_PLATFORM: 'windows' })).toBe(false);
    expect(isMobileShell({})).toBe(false);
  });

  it('lets VITE_MOONY_SHELL override the platform in both directions', () => {
    expect(isMobileShell({ TAURI_ENV_PLATFORM: 'macos', VITE_MOONY_SHELL: 'mobile' })).toBe(true);
    expect(isMobileShell({ TAURI_ENV_PLATFORM: 'ios', VITE_MOONY_SHELL: 'desktop' })).toBe(false);
    expect(isMobileShell({ VITE_MOONY_SHELL: 'mobile' })).toBe(true);
  });

  it('ignores an unknown VITE_MOONY_SHELL value', () => {
    expect(isMobileShell({ TAURI_ENV_PLATFORM: 'ios', VITE_MOONY_SHELL: 'tablet' })).toBe(true);
    expect(isMobileShell({ TAURI_ENV_PLATFORM: 'macos', VITE_MOONY_SHELL: '' })).toBe(false);
  });
});
