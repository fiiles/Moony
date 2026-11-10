import { beforeEach, describe, expect, it, vi } from 'vitest';

const plugin = vi.hoisted(() => ({
  checkPermissions: vi.fn(),
  requestPermissions: vi.fn(),
  scan: vi.fn(),
  cancel: vi.fn(),
  openAppSettings: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-barcode-scanner', () => ({
  ...plugin,
  Format: { QRCode: 'QR_CODE' },
}));

import { cancelQrScan, scanQrCode } from './qr-scanner';

/** A promise and the function that settles it. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

describe('scanQrCode', () => {
  beforeEach(() => {
    for (const fn of Object.values(plugin)) fn.mockReset();
    plugin.cancel.mockResolvedValue(undefined);
  });

  it('scans one QR code, windowed, once the camera is allowed', async () => {
    plugin.checkPermissions.mockResolvedValue('granted');
    plugin.scan.mockResolvedValue({ content: 'moony-pair:1:x', format: 'QR_CODE' });
    await expect(scanQrCode()).resolves.toEqual({ kind: 'code', text: 'moony-pair:1:x' });
    expect(plugin.scan).toHaveBeenCalledWith({ windowed: true, formats: ['QR_CODE'] });
    expect(plugin.requestPermissions).not.toHaveBeenCalled();
  });

  it('never opens the camera when cancelled while the permission prompt was up', async () => {
    plugin.checkPermissions.mockResolvedValue('prompt');
    const asked = deferred<string>();
    plugin.requestPermissions.mockReturnValue(asked.promise);
    const outcome = scanQrCode();
    await vi.waitFor(() => expect(plugin.requestPermissions).toHaveBeenCalled());
    await cancelQrScan();
    asked.resolve('granted');
    await expect(outcome).resolves.toEqual({ kind: 'cancelled' });
    expect(plugin.scan).not.toHaveBeenCalled();
  });

  it('starts afresh after an earlier cancel', async () => {
    await cancelQrScan();
    plugin.checkPermissions.mockResolvedValue('granted');
    plugin.scan.mockResolvedValue({ content: 'text', format: 'QR_CODE' });
    await expect(scanQrCode()).resolves.toEqual({ kind: 'code', text: 'text' });
  });

  it('says denied when the camera is refused, and cancelled when the scan is', async () => {
    plugin.checkPermissions.mockResolvedValue('prompt');
    plugin.requestPermissions.mockResolvedValue('denied');
    await expect(scanQrCode()).resolves.toEqual({ kind: 'denied' });

    plugin.checkPermissions.mockResolvedValue('granted');
    plugin.scan.mockRejectedValue('cancelled');
    await expect(scanQrCode()).resolves.toEqual({ kind: 'cancelled' });
  });

  it('rejects any other failure (no camera)', async () => {
    plugin.checkPermissions.mockResolvedValue('granted');
    plugin.scan.mockRejectedValue('No camera available on this device (e.g., iOS Simulator)');
    await expect(scanQrCode()).rejects.toBe(
      'No camera available on this device (e.g., iOS Simulator)'
    );
  });
});
