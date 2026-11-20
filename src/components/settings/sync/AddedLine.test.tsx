import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import type { RemovalPlanDevice } from '@shared/schema';
import { AddedLine } from './AddedLine';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      `${key}|${Object.values(values ?? {}).join('|')}`,
  }),
}));
vi.mock('@/lib/use-format', () => ({
  useFormat: () => ({ date: (value: Date) => `D${value.getTime() / 1000}` }),
}));

const device = (over: Partial<RemovalPlanDevice>): RemovalPlanDevice => ({
  id: 'd',
  name: 'Phone',
  platform: 'ios',
  joinedAt: 111,
  thisDevice: false,
  addedAt: 222,
  addedBy: 'Mac',
  ...over,
});

const html = (d: RemovalPlanDevice) => renderToStaticMarkup(<AddedLine device={d} className="x" />);

describe('AddedLine', () => {
  it('shows the date this device first verified the device, with who added it', () => {
    expect(html(device({}))).toContain('remove.addedRow|D222|Mac');
  });

  it('shows the date alone when the adding device is not known', () => {
    expect(html(device({ addedBy: null }))).toContain('remove.addedRowNoIssuer|D222');
  });

  it("never shows the certificate's date as the date it was added", () => {
    const out = html(device({ addedAt: null }));
    expect(out).toBe('');
    expect(out).not.toContain('111');
  });
});
