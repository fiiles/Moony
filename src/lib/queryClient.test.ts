import { describe, expect, it } from 'vitest';
import { queryClient } from './queryClient';

describe('queryClient defaults', () => {
  // Every query and mutation is a local Tauri IPC call, never a network request. TanStack Query's
  // default ('online') pauses them all after the webview's `offline` event, which froze the app
  // (a phone import sheet on "reading", Cashflow on skeletons) until a reload.
  it('runs queries whether or not the machine is online', () => {
    expect(queryClient.getDefaultOptions().queries?.networkMode).toBe('always');
  });

  it('runs mutations whether or not the machine is online', () => {
    expect(queryClient.getDefaultOptions().mutations?.networkMode).toBe('always');
  });

  // 'always' turns refetchOnReconnect off unless it is set; a stale query whose command reaches
  // the network (a stock's company info) must still refresh when the connection returns.
  it('refreshes stale queries when the connection returns', () => {
    expect(queryClient.getDefaultOptions().queries?.refetchOnReconnect).toBe(true);
  });
});
