import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { setStatusBarStyle } = vi.hoisted(() => ({ setStatusBarStyle: vi.fn() }));

vi.mock('@/lib/tauri-api', () => ({ keychainApi: { setStatusBarStyle } }));

import { applyStatusBarStyle } from './status-bar';

describe('applyStatusBarStyle', () => {
  beforeEach(() => {
    setStatusBarStyle.mockReset();
    setStatusBarStyle.mockResolvedValue(undefined);
  });
  afterEach(() => vi.unstubAllGlobals());

  it('hands the dark theme to the native status bar inside Tauri', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    await applyStatusBarStyle('dark');
    expect(setStatusBarStyle).toHaveBeenCalledTimes(1);
    expect(setStatusBarStyle).toHaveBeenCalledWith('dark');
  });

  it('hands the light theme to the native status bar inside Tauri', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    await applyStatusBarStyle('light');
    expect(setStatusBarStyle).toHaveBeenCalledTimes(1);
    expect(setStatusBarStyle).toHaveBeenCalledWith('light');
  });

  it('does nothing outside Tauri', async () => {
    vi.stubGlobal('window', {});
    await applyStatusBarStyle('dark');
    expect(setStatusBarStyle).not.toHaveBeenCalled();
  });

  it('does nothing without a window at all', async () => {
    vi.stubGlobal('window', undefined);
    await applyStatusBarStyle('light');
    expect(setStatusBarStyle).not.toHaveBeenCalled();
  });

  it('swallows a rejection: the page already follows the theme', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    setStatusBarStyle.mockRejectedValue(new Error('not allowed'));
    await expect(applyStatusBarStyle('dark')).resolves.toBeUndefined();
    expect(setStatusBarStyle).toHaveBeenCalledWith('dark');
  });

  it('swallows a synchronous throw as well', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    setStatusBarStyle.mockImplementation(() => {
      throw new Error('no ipc');
    });
    await expect(applyStatusBarStyle('light')).resolves.toBeUndefined();
  });
});
